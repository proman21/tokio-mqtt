use std::error;
use std::convert::AsRef;
use std::future::Future;

use snafu::ResultExt;
use tokio::sync::Mutex;
use tokio::io::{AsyncRead, AsyncWrite};
use mqtt3_proto::{
    QualityOfService,
    MqttPacket,
    PublishType,
    MqttString,
    PacketType,
    SubscriptionTuple,
    SubAckReturnCode,
};

use crate::persistence::Persistence;
use crate::config::Config;
use crate::backend::{Reactor, ReactorClient};
use crate::backend::channel::{LoopSender, Subscription};
use crate::MqttResult;
use crate::errors::*;

fn get_persistence_key(id: u16, ty: PacketType) -> String {
    format!("{}_{}", ty, id)
}

/// Interface to the MQTT client. All futures and streams returned are `Send`, allowing subscription
/// handling from different threads.
pub struct Client<'a, P: Persistence<'a>> {
    sender: LoopSender<'a>,
    id: Mutex<u16>,
    persist: Mutex<P>
}

impl<'a, P: Persistence<'a>> Client<'a, P> {
    /// Create a client-reactor pair.
    pub fn new(persistence: P) -> (Client<'a, P>, Reactor<'a>) {
        unimplemented!()
    }

    /// Starts an MQTT session with the provided configuration. If successful, returns a `Subscription<'a>` that
    /// represents lost messages, that is all messages that don't match any current subscriptions.
    ///
    /// `io_read` and `io_write` should be some network socket that provides an ordered, lossless stream of bytes.
    /// TCP, TLS and Websockets are all supported standards, however you may choose any protocol
    /// that fits the above criteria. The client can not guarantee QoS compliance or good service if
    /// these criteria aren't met. The client is expected to own a unique reference of `io` that it can
    /// disconnect at will without disrupting other references to the underlying socket.
    ///
    /// `config` provides options to configure the client.
    pub fn connect(&mut self, io_read: &'a mut dyn AsyncRead, io_write: &'a mut dyn AsyncWrite, cfg: &'a Config<'a>) -> impl Future<Output = MqttResult<'a, Subscription<'a>>> {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async move {
            rctr_client.connect(io_read, io_write, cfg).await
        }
    }

    /// Issues a disconnect packet to the server and closes the network connection. The client will
    /// wait for all QoS1 and QoS2 messages to be fully processed, unless `timeout` is specified,
    /// in which case the client will only wait until the timeout value has passed. The future
    /// returned will resolve to a bool; true means all packets were processed before disconnect,
    /// false meaning the timeout occurred before work could finish. All streams will still recieve
    /// packets until the the disconnect packet is issued
    pub fn disconnect<T: Into<Option<u64>>>(&'a mut self, timeout: T) -> impl Future<Output = MqttResult<'a, bool>> {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async move {
            rctr_client.disconnect(timeout.into()).await
        }
    }

    /// Publish a message to a particular topic with a particular QoS level. Returns a future that
    /// resolves when the publish QoS flow completes.
    pub fn publish<B>(&'a mut self, topic: &'a str, qos: QualityOfService, retain: bool, msg: B) -> impl Future<Output = MqttResult<'a, ()>>
    where B: AsRef<[u8]> + 'a
    {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async {
            let topic_name = MqttString::new(topic)?;
            match qos {
                QualityOfService::QoS0 => {
                    rctr_client.send_packet(MqttPacket::Publish {
                        pub_type: PublishType::QoS0,
                        topic_name,
                        message: msg.as_ref(),
                        retain,
                    }).await
                },
                QualityOfService::QoS1 => {
                    let packet_id = self.get_packet_id().await;
                    let persist_key = format!("{}_{}", packet_id, packet_id);
                    let packet = MqttPacket::Publish {
                        pub_type: PublishType::QoS1 {
                            packet_id,
                            dup: false
                        },
                        topic_name,
                        message: msg.as_ref(),
                        retain,
                    };
                    self.persist.lock().await.put(&persist_key, &packet)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError);
                    rctr_client.send_packet(packet).await?;
                    rctr_client.await_response((PacketType::PubAck, packet_id)).await?;
                    self.persist.lock().await.remove(&persist_key)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError);
                    Ok(())
                },
                QualityOfService::QoS2 => {
                    let packet_id = self.get_packet_id().await;
                    let persist_key = get_persistence_key(packet_id, PacketType::Publish);
                    let packet = MqttPacket::Publish {
                        pub_type: PublishType::QoS1 {
                            packet_id,
                            dup: false
                        },
                        topic_name,
                        message: msg.as_ref(),
                        retain,
                    };

                    // Send Publish message.
                    self.persist.lock().await.put(&persist_key.clone(), &packet)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError)?;
                    rctr_client.send_packet(packet).await?;

                    // Qwait Publish Receive message.
                    rctr_client.await_response((PacketType::PubRec, packet_id)).await?;
                    self.persist.lock().await.remove(&persist_key)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError)?;

                    // Send Publish Release messsage.
                    let pub_rel = MqttPacket::PubRel{ packet_id };
                    let pub_rel_key = get_persistence_key(packet_id, PacketType::PubRel);
                    self.persist.lock().await.put(&pub_rel_key, &packet)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError)?;
                    rctr_client.send_packet(pub_rel).await?;

                    // Await Publish Complete message.
                    rctr_client.await_response((PacketType::PubComp, packet_id)).await?;
                    self.persist.lock().await.remove(&pub_rel_key)
                        .map_err::<Box<dyn error::Error>, _>(|e| Box::new(e))
                        .context(PersistenceError)?;

                    Ok(())
                },
            }
        }
    }

    /// Subscribe to a particular topic filter. This returns a Future which resolves to a `Stream`
    /// of messages that match the provided filter; the string provided will contain the topic
    /// string the subscription matched on. The stream will stop when `unsubscribe` is called with
    /// the matching topic filter or when the client disconnects.
    pub fn subscribe(&'a mut self, topic: &'a String, qos: QualityOfService) -> impl Future<Output=MqttResult<Subscription<'a>>>
    {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async {
            let packet_id = self.get_packet_id().await;
            let packet = MqttPacket::Subscribe {
                packet_id,
                subscriptions: vec![SubscriptionTuple(
                    MqttString::new(&topic).context(ProtocolError)?,
                    qos
                )],
            };
            rctr_client.send_packet(packet).await?;
            let sub_resp = rctr_client.await_response((PacketType::SubAck, packet_id)).await?;
            
            if let MqttPacket::SubAck { results, .. } = *sub_resp {
                if let SubAckReturnCode::Failure = results[0] {
                    SubscriptionRejected {
                        topic,
                        qos
                    }.fail()
                } else {
                    rctr_client.subscribe(topic).await
                    
                }
            } else {
                unreachable!()
            }
        }
    }

    /// Unsubscribe from a topic, causing the associated stream to terminate. Returns a future
    /// that resolves when the request is acknowledged.
    pub fn unsubscribe(&'a mut self, topic: &'a String) -> impl Future<Output = MqttResult<'a, ()>> {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async {
            let packet_id = self.get_packet_id().await;
            let packet = MqttPacket::Unsubscribe {
                packet_id,
                topics: vec![MqttString::new(&topic).context(ProtocolError)?],
            };

            rctr_client.send_packet(packet).await?;
            rctr_client.await_response((PacketType::UnsubAck, packet_id)).await?;
            rctr_client.unsubscribe(topic).await
        }
    }

    /// Ping the server to check it's still available. Returns a future that resolves when the
    /// server responds.
    ///
    /// The client will pause automatic pinging while this request is processed. This means that
    /// if the packet is lost in transit, and the server doesn't respond within the keep alive
    /// window, the client will assume the server or connection is down and will disconnect,
    /// returning an error. Be aware that this scenario will trigger the sending of the Last Will
    /// and Testament message specified by this client.
    pub fn ping(&mut self) -> impl Future<Output = MqttResult<'a, ()>> {
        let rctr_client = ReactorClient::new(self.sender.clone());
        async move {
            rctr_client.send_packet(MqttPacket::PingReq).await?;
            rctr_client.await_response(PacketType::PingResp).await?;
            Ok(())
        }
    }

    fn get_packet_id(&'a self) -> impl Future<Output = u16> + 'a {
        async {
            let mut guard = self.id.lock().await;
            let id = *guard;
            guard.wrapping_add(1);
            id
        }
    }
}
