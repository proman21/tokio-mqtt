use std::collections::hash_map::Entry;
use std::ops::Deref;
use std::result;

use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Stream, Future};
use ::futures::stream::StreamFuture;
use ::futures::sync::mpsc::{unbounded};
use ::bytes::Bytes;
use ::take::Take;
use ::futures_mutex::{FutMutex, FutMutexGuard};
use ::bincode;

use ::persistence::Persistence;
use ::proto::{
    MqttPacket,
    PacketType,
    Headers,
    QualityOfService,
    Payload,
    ConnectReturnCode,
    ConnectAckFlags,
    PacketId,
    TopicName
};
use ::errors::{Result, Error, ErrorKind, ResultExt};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::types::SubItem;
use super::MqttFramedReader;
use super::mqtt_loop::LoopData;
use super::{
    SubscriptionResult,
    SourceItem,
    SourceError,
    ClientReturn,
    OneTimeKey,
    TopicFilter,
    PublishState
};

enum State {
    Processing(MqttPacket),
    Sending(MqttPacket),
    Writing
}

/// This type will take a packet from the server and process it, returning the stream it came from
pub struct ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    state: Take<State>,
    data: FutMutex<LoopData<I, P>>
}

impl<I, P> ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    pub fn new(packet: MqttPacket, data: FutMutex<LoopData<I, P>>) -> ResponseProcessor<I, P> {
        ResponseProcessor {
            state: Take::new(State::Processing(packet)),
            data: data,
        }
    }

    fn process_conn_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        // Check if we are awaiting a CONNACK
        let (_, client) = match data.one_time.entry(OneTimeKey::Connect) {
            Entry::Vacant(v) => {
                return Err(SourceError::ProcessResponse(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(packet.ty.clone())
                ).into()));
            },
            Entry::Occupied(o) => o.remove()
        };
        // Validate connect return code
        let crc = packet.headers.get::<ConnectReturnCode>().unwrap();
        if crc.is_ok() {
            // If session is not clean, setup a stream.
            let sess = packet.headers.get::<ConnectAckFlags>().unwrap();
            if sess.is_clean() {
                let _ = client.send(Ok(ClientReturn::Onetime(Some(packet))));
            } else {
                let (tx, rx) = unbounded::<SubscriptionResult>();
                let custom_rx = rx.map_err(|_| {
                    Error::from(ErrorKind::LoopCommsError)
                });
                data.session_subs = Some(tx);
                let _ = client.send(Ok(ClientReturn::Ongoing(vec![
                    Ok((custom_rx.boxed(), QualityOfService::QoS0))
                ])));
            }
            Ok(None)
        } else {
            return Err(SourceError::ProcessResponse(ErrorKind::from(
                ProtoErrorKind::ConnectionRefused(*crc)).into()
            ))
        }
    }

    fn process_sub_ack(mut data: FutMutexGuard<LoopData<I,P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let packet_id = packet.headers.get::<PacketId>().unwrap();
        let (o, c) = match data.one_time.entry(OneTimeKey::Subscribe(*packet_id)) {
            Entry::Vacant(v) => {
                return Err(SourceError::ProcessResponse(ErrorKind::from(
                    ProtoErrorKind::UnexpectedResponse(packet.ty.clone())).into()));
            },
            Entry::Occupied(o) => o.remove()
        };
        // Validate each return code
        let orig = match o.payload {
            Payload::Subscribe(v) => v,
            _ => unreachable!()
        };
        let ret_codes = match packet.payload {
            Payload::SubAck(v) => v,
            _ => unreachable!()
        };
        let mut collect = Vec::new();
        for (sub, ret) in orig.iter().zip(ret_codes.iter()) {
            if ret.is_ok() {
                let (tx, rx) = unbounded::<SubscriptionResult>();
                let custom_rx = rx.map_err(|_| {
                    Error::from(ErrorKind::LoopCommsError)
                });
                let filter = TopicFilter::from_string(&sub.topic).map_err(|e| {
                    SourceError::ProcessResponse(e)
                })?;
                data.subscriptions.insert(sub.topic.clone().into(), (filter, tx));
                collect.push(Ok((custom_rx.boxed(), sub.qos)));
            } else {
                collect.push(Err(ErrorKind::from(
                    ProtoErrorKind::SubscriptionRejected(
                        sub.topic.clone().into(), sub.qos)
                    ).into()
                ));
            }
        }
        let _ = c.send(Ok(ClientReturn::Ongoing(collect)));
        Ok(None)
    }

    fn process_unsub_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let pid = packet.headers.get::<PacketId>().unwrap();
        let (o, c) = match data.one_time.entry(OneTimeKey::Unsubscribe(*pid)) {
            Entry::Vacant(v) => {
                return Err(SourceError::ProcessResponse(ErrorKind::from(
                    ProtoErrorKind::UnexpectedResponse(packet.ty.clone())).into()));
            },
            Entry::Occupied(o) => o.remove()
        };
        let topics = match o.payload {
            Payload::Unsubscribe(v) => v,
            _ => unreachable!()
        };
        // Remove subscriptions from loop
        for topic in topics {
            let _ = data.subscriptions.remove(topic.deref());
        }
        let _ = c.send(Ok(ClientReturn::Onetime(None)));
        Ok(None)
    }

    fn process_ping_resp(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        if let Entry::Occupied(o) = data.one_time.entry(
            OneTimeKey::PingReq) {
            let (_, client) = o.remove();
            let _ = client.send(Ok(ClientReturn::Onetime(None)));
        }
        Ok(None)
    }

    fn process_pub(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        // Scream for the publish
        match packet.flags.qos() {
            QualityOfService::QoS0 => {
                let topic = packet.headers.get::<TopicName>().unwrap();
                let payload = match packet.payload {
                    Payload::Application(d) => Bytes::from(d),
                    _ => unreachable!()
                };
                for &(ref filter, ref sender) in data.subscriptions.values() {
                    if filter.match_topic(&topic) {
                        let _ = sender.send(Ok((topic.clone().into(), payload.clone())));
                    }
                }
                Ok(None)
            },
            QualityOfService::QoS1 => {
                let id = packet.headers.get::<PacketId>().unwrap();

                let topic = packet.headers.get::<TopicName>().unwrap();
                let payload = match packet.payload {
                    Payload::Application(d) => Bytes::from(d),
                    _ => unreachable!()
                };
                for &(ref filter, ref sender) in data.subscriptions.values() {
                    if filter.match_topic(&topic) {
                        let _ = sender.send(Ok((topic.clone().into(), payload.clone())));
                    }
                }

                // Send back an acknowledgement
                Ok(Some(MqttPacket::pub_ack_packet(*id)))
            },
            QualityOfService::QoS2 => {
                let id = packet.headers.get::<PacketId>().unwrap();

                // Check if we have an existing publish with the same id
                if data.server_publish_state.contains_key(&id) {
                    return Err(SourceError::ProcessResponse(ProtoErrorKind::QualityOfServiceError(
                        packet.flags.qos(),
                        format!("Duplicate publish recieved with same Packet ID: {}", *id)
                    ).into()))
                } else {
                    data.server_publish_state.insert(*id, PublishState::Received(packet));
                    // Send PUBREC
                    Ok(Some(MqttPacket::pub_rec_packet(*id)))
                }
            }
        }
    }

    fn process_pub_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let id = packet.headers.get::<PacketId>().unwrap();
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Sent(p, s)) => (p, s),
                _ => unreachable!()
            };
            match data.persistence.remove(p_key) {
                Ok(_) => {
                    if let Some(s) = sender {
                        let _ = s.send(Ok(ClientReturn::Onetime(None)));
                    }
                },
                Err(e) => {
                    if let Some(s) = sender {
                        let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                    }
                    return Err(ErrorKind::PersistenceError.into())
                        .map_err(|e| SourceError::ProcessResponse(e))
                }
            }
            Ok(None)
        } else {
            bail!(SourceError::ProcessResponse(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_rec(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let id = packet.headers.get::<PacketId>().unwrap();
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Sent(p, s)) => (p, s),
                _ => unreachable!()
            };
            if let Err(e) = data.persistence.remove(p_key) {
                return Err(ErrorKind::PersistenceError.into())
                    .map_err(|e| SourceError::ProcessResponse(e))
            }
            let rel = MqttPacket::pub_rel_packet(*id);
            let ser = bincode::serialize(&rel, bincode::Infinite).unwrap();
            let key = match data.persistence.append(ser) {
                Ok(k) => k,
                Err(e) => {
                    if let Some(s) = sender {
                        let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                    }
                    return Err(ErrorKind::PersistenceError.into())
                        .map_err(|e| SourceError::ProcessResponse(e))
                }
            };
            let _ = data.client_publish_state.insert(*id, PublishState::Released(key, sender));

            Ok(Some(rel))
        } else {
            bail!(SourceError::ProcessResponse(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_rel(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let id = packet.headers.get::<PacketId>().unwrap();
        if data.server_publish_state.contains_key(&id) {
            let orig = match data.server_publish_state.remove(&id) {
                Some(PublishState::Received(o)) => o,
                _ => unreachable!()
            };

            let topic = orig.headers.get::<TopicName>().unwrap();
            let payload = match orig.payload {
                Payload::Application(d) => Bytes::from(d),
                _ => unreachable!()
            };
            for &(ref filter, ref sender) in data.subscriptions.values() {
                if filter.match_topic(&topic) {
                    let _ = sender.send(Ok((topic.clone().into(), payload.clone())));
                }
            }

            Ok(Some(MqttPacket::pub_comp_packet(*id)))
        } else {
            bail!(SourceError::ProcessResponse(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_comp(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        let id = packet.headers.get::<PacketId>().unwrap();
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Released(p, s)) => (p, s),
                _ => unreachable!()
            };
            match data.persistence.remove(p_key) {
                Ok(_) => {
                    if let Some(s) = sender {
                        let _ = s.send(Ok(ClientReturn::Onetime(None)));
                    }
                },
                Err(e) => {
                    if let Some(s) = sender {
                        let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                    }
                    return Err(ErrorKind::PersistenceError.into())
                        .map_err(|e| SourceError::ProcessResponse(e))
                }
            }
            Ok(None)
        } else {
            bail!(SourceError::ProcessResponse(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }
}

impl<I, P> Future for ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    type Item = SourceItem<I>;
    type Error = SourceError<I>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop { match self.state.take() {
            State::Processing(packet) => {
                // Get a lock on the data
                let data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Processing(packet));
                        return Ok(Async::NotReady)
                    }
                };
                packet.validate().map_err(|e| SourceError::ProcessResponse(e))?;
                let state = match packet {
                    p @ MqttPacket{ty: PacketType::ConnAck, ..} =>{
                        ResponseProcessor::process_conn_ack(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::SubAck, ..} => {
                        ResponseProcessor::process_sub_ack(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::UnsubAck, ..} => {
                        ResponseProcessor::process_unsub_ack(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::PingResp, ..} => {
                        ResponseProcessor::process_ping_resp(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::Publish, ..} => {
                        ResponseProcessor::process_pub(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::PubAck, ..} => {
                        ResponseProcessor::process_pub_ack(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::PubRec, ..} => {
                        ResponseProcessor::process_pub_rec(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::PubRel, ..} => {
                        ResponseProcessor::process_pub_rel(data, p)?
                    },
                    p @ MqttPacket{ty: PacketType::PubComp, ..} => {
                        ResponseProcessor::process_pub_comp(data, p)?
                    },
                    p @ _ => return Err(SourceError::ProcessResponse(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(p.ty.clone())
                    ).into()))
                };
                if let Some(s) = state {
                    self.state = Take::new(State::Sending(s));
                } else {
                    return Ok(Async::Ready(SourceItem::ProcessResponse(false)));
                }
            },
            State::Sending(packet) => {
                let mut data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Sending(packet));
                        return Ok(Async::NotReady)
                    }
                };
                self.state = Take::new(match data.framed_write.start_send(packet) {
                    Ok(AsyncSink::Ready) => State::Writing,
                    Ok(AsyncSink::NotReady(p)) => State::Sending(p),
                    Err(e) => return Err(SourceError::ProcessResponse(e))
                })
            },
            State::Writing => {
                let mut data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Writing);
                        return Ok(Async::NotReady)
                    }
                };
                try_ready!(data.framed_write.poll_complete()
                    .map_err(|e| SourceError::ProcessResponse(e)));
                return Ok(Async::Ready(SourceItem::ProcessResponse(true)));
            }
        };}
    }
}
