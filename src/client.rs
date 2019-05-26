use std::default::Default;

use tokio_core::reactor::Remote;
use tokio_io::{AsyncRead, AsyncWrite};
use bytes::Bytes;
use futures::prelude::*;
use futures::{future, sync, unsync};
use slog::Logger;
use slog_stdlog;
use mqtt3_proto::{MqttPacket, ProtoLvl, LWTMessage};

use backend::{Loop, MqttCodec, ClientReturn, Connect, LoopMessage};
use types::{MqttFuture, Subscription};
use errors::{Result as MqttResult, Error, ErrorKind};
use persistence::Persistence;

/// Used to configure the client. Defaults are as follows
///  - Connect Timeout: 30 seconds
///  - Ping Timeout: 10 seconds
///  - Keep Alive: 0 (off)
///  - In-flight Window: 0 (off)
///  - Version: MQTT 3.1.1
///  - Credentials: None
///  - Last Will and Testament: None
///  - Clean Session: true
///  - Client ID: empty
///  - Logger: `log`
#[derive(Clone, Builder)]
#[builder(setter(into), build_fn(validate = "validate"))]
pub struct Config {
    /// Specify how long the client should wait for the server to acknowledge a connection request.
    /// 0 means wait indefinitely.
    #[builder(default = "30")]
    pub connect_timeout: u64,
    /// Specify how long the client should wait for a ping response.
    /// 0 means wait indefinitely.
    #[builder(default = "10")]
    pub ping_timeout: u64,
    /// Sets the keep alive value of the connection. If no response is received in `k * 1.5`
    /// seconds, the server will treat the client as disconnected. The client will send pings to
    /// the server every `k` seconds when idle.
    #[builder(default = "0")]
    pub keep_alive: u16,
    /// Specify which version of the MQTT protocol to use when communicating with the server.
    #[builder(default = "ProtoLvl::V3_1_1")]
    pub version: ProtoLvl,
    /// Sets the Last Will and Testament message. This is stored by the server and sent to the
    /// specified topic in the event that:
    ///  - The network connection fails
    ///  - The client fails to communicate before the keep-alive time expires
    ///  - The client closes the connection without sending a disconnect message
    ///  - A protocol error occurs
    #[builder(default = "None")]
    pub lwt: Option<LWTMessage<String, Vec<u8>>,
    /// Specify the credentials the client will use when connecting to the server.
    #[builder(default = "None")]
    pub credentials: Option<Credentials<String< Vec<u8>>>,
    /// Specify whether the server should treat this session as clean.
    #[builder(default = "true")]
    pub clean: bool,
    /// Set the ID used to identify this client to the server. If clean is false and this client
    /// has a session stored on the server, you must set this value to the ID used in past sessions.
    #[builder(default = "None")]
    pub client_id: Option<String>,
    /// Specify a logger for the client. Defaults to passing to `log`
    #[builder(default = "self.default_logger()?")]
    pub logger: Logger
}

impl ConfigBuilder {
    fn default_logger(&self) -> Result<Logger, String> {
        Ok(Logger::root(slog_stdlog::StdLog.fuse(), o!()))
    }

    fn validate(&self) -> Result<(), String> {
        if self.clean && self.client_id.is_none() {
            Err("Please specify a client ID for a non-clean session".to_string())
        } else {
            Ok(())
        }
    }
}

/// Describes the current state of the client.
pub enum ClientState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected
}

/// Interface to the MQTT client. All futures and streams returned are `Send`, allowing subscription
/// handling from different threads.
pub struct Client<P: 'static> where P: Persistence {
    sender: LoopMpscSender<LoopMessage>,
    state: ClientState
}

impl<P: 'static> Client<P> where P: Persistence + Send {
    /// Return an configuration builder object. See `ConfigBuilder` for instructions on how to use.
    pub fn config() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Create a new client using the specified persistence store.
    pub fn new(persist: P) -> Client<P> {

    }

    /// Return the current state of the client.
    pub fn client_state(&self) -> &ClientState {
        self.state
    }

    /// Starts an MQTT session with the provided configuration.
    ///
    /// `io` should be some network socket that provides an ordered, lossless stream of bytes.
    /// TCP, TLS and Websockets are all supported standards, however you may choose any protocol
    /// that fits the above criteria. The client can not guarantee QoS compliance or good service if
    /// these criteria aren't met. The client is expected to own a unique value of `io` that it can
    /// drop at will without disrupting other references to the underlying socket.
    ///
    /// If the server has a previous session with this client, and Clean Session has been set to
    /// false, the returned stream will contain all the messages that this client missed, based on
    /// its previous subscriptions.
    ///
    /// `config` provides options to configure the client.
    ///
    /// `reactor` is a remote handle to a `tokio_core::Reactor`. If the core is on the same thread,
    /// the client will optimise to use single-threaded communication.
    pub fn connect<I>(&mut self, io: I, cfg: &Config, reactor: &Remote) -> MqttResult<Option<Subscription>> {
        if let Some(_) = self.loop_address {
            bail!(ErrorKind::AlreadyConnected)
        }

        // Prepare a connect packet to send using the provided values
        let lwt = if let Some((ref t, ref q, ref r, ref m)) = cfg.lwt {
         let topic = MqttString::from_str(&t)?;
         Some(LWTMessage::new(topic, q.clone(), *r, m.clone()))
        } else {
         None
        };
        let client_id = if let Some(ref cid) = cfg.client_id {
         Some(MqttString::from_str(&cid)?)
        } else {
         None
        };
        let cred = if let Some((ref user, ref p)) = cfg.credentials {
         let pwd = if let &Some(ref pass) = p {
             Some(MqttString::from_str(pass)?)
         } else {
             None
         };
         Some((MqttString::from_str(&user)?, pwd))
        } else {
         None
        };
        let connect_msg = Connect::new(
            MqttPacket::connect_packet(cfg.version, lwt, cred, cfg.clean, cfg.keep_alive,
                                       client_id),
            Some(cfg.connect_timeout).and_then(|t| if t == 0 { None } else { Some(t) })
        );

        let config = cfg.clone();


        match self.loop_address {
            Some(ref addr) => {
                let res = addr.call_fut(connect_msg).flatten().flatten();

                match res.wait()? {
                    ClientReturn::Onetime(_) => Ok(None),
                    ClientReturn::Ongoing(mut subs) => {
                        match subs.pop() {
                            Some(Ok((s, _))) =>  Ok(Some(s)),
                            _ => unreachable!()
                        }
                    }
                }
            },
            None => unreachable!()
        }
    }

    /// Issues a disconnect packet to the server and closes the network connection. The client will
    /// wait for all QoS1 and QoS2 messages to be fully processed, unless `timeout` is specified,
    /// in which case the client will only wait until the timeout value has passed. The future
    /// returned will resolve to a bool; true means all packets were processed before disconnect,
    /// false meaning the timeout occurred before work could finish. All streams will still recieve
    /// packets until the the disconnect packet is issued
    pub fn disconnect<T: Into<Option<u64>>>(&mut self, timeout: T) -> MqttFuture<bool> {
        unimplemented!()
    }

    /// Publish a message to a particular topic with a particular QoS level. Returns a future that
    /// resolves when the publish QoS flow completes.
    pub fn publish(&mut self, topic: String, qos: QualityOfService, retain: bool, msg: Bytes) -> MqttFuture<()> {
        unimplemented!()
    }

    /// Subscribe to a particular topic filter. This returns a Future which resolves to a `Stream`
    /// of messages that match the provided filter; the string provided will contain the topic
    /// string the subscription matched on. The stream will stop when `unsubscribe` is called with
    /// the matching topic filter or when the client disconnects.
    pub fn subscribe(&mut self, topic: String, qos: QualityOfService) -> MqttFuture<Subscription> {
        unimplemented!()
    }

    /// Subscribes to multiple topic filters at once. This returns a `Vec` of `Future`'s that
    /// resolve to `Stream`'s of messages matching the corresponding topic filters. The streams
    /// will stop when `unsubscribe` is called with the matching topic filter or the client
    /// disconnects.
    pub fn subscribe_many(&mut self, subscriptions: Vec<(String, QualityOfService)>) ->  Vec<MqttFuture<SubStream>>{
        unimplemented!()
    }

    /// Unsubscribe from a topic, causing the associated stream to terminate. Returns a future
    /// that resolves when acknowledged.
    pub fn unsubscribe(&mut self, topic: String) -> MqttFuture<()> {
        unimplemented!()
    }

    /// Unsubscribe from multiple topics, causing the associated streams to terminate. Returns a
    /// future that resolves when acknowledged.
    pub fn unsubscribe_many(&mut self, topics: Vec<String>) -> MqttFuture<()> {
        unimplemented!()
    }

    /// Ping the server to check it's still available. Returns a future that resolves when the
    /// server responds.
    ///
    /// The client will pause automatic pinging while this request is processed. This means that
    /// if the packet is lost in transit, and the server doesn't respond within the keep alive
    /// window, the client will assume the server or connection is down and will disconnect,
    /// returning an error. Be aware that this scenario will trigger the sending of the Last Will
    /// and Testament message specified by this client.
    pub fn ping(&mut self) -> MqttFuture<()> {
        unimplemented!()
    }
}
