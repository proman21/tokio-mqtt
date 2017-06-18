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
use ::proto::{MqttPacket, PacketType, Headers, QualityOfService, Payload};
use ::errors::{Result, Error, ErrorKind, ResultExt};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::types::SubItem;
use super::MqttFramedReader;
use super::mqtt_loop::LoopData;
use super::{SourceItem, SourceError, ClientReturn, OneTimeKey, TopicFilter, PublishState};

enum State {
    Reading,
    Processing(MqttPacket),
    Sending(MqttPacket),
    Writing
}

/// This type will take a packet from the server and process it, returning the stream it came from
pub struct ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    state: Take<State>,
    data: FutMutex<LoopData<I, P>>,
    future: StreamFuture<MqttFramedReader<I>>,
    stream: Option<MqttFramedReader<I>>
}

impl<I, P> ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    pub fn new(future: StreamFuture<MqttFramedReader<I>>, data: FutMutex<LoopData<I, P>>) -> ResponseProcessor<I, P> {
        ResponseProcessor {
            state: Take::new(State::Reading),
            data: data,
            future: future,
            stream: None
        }
    }

    fn process_conn_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        // Check if we are awaiting a CONNACK
        let (_, client) = match data.one_time.entry(OneTimeKey::Connect) {
            Entry::Vacant(v) => {
                return Err(SourceError::Response(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(packet.ty.clone())
                ).into()));
            },
            Entry::Occupied(o) => o.remove()
        };
        // Validate connect return code
        let crc = match packet.headers.get("connect_return_code").unwrap() {
            &Headers::ConnRetCode(crc) => crc,
            _ => unreachable!()
        };
        if crc.is_ok() {
            // If session is not clean, setup a stream.
            let sess = match packet.headers.get("connect_ack_flags").unwrap() {
                &Headers::ConnAckFlags(fl) => fl,
                _ => unreachable!()
            };
            if sess.is_clean() {
                let _ = client.send(Ok(ClientReturn::Onetime(Some(packet))));
            } else {
                let (tx, rx) = unbounded::<Result<SubItem>>();
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
            let _ = client.send(Err(ErrorKind::from(
                ProtoErrorKind::ConnectionRefused(crc)).into()));
            return Err(SourceError::Response(ErrorKind::from(
                ProtoErrorKind::ConnectionRefused(crc.clone())).into()
            ))
        }
    }

    fn process_sub_ack(mut data: FutMutexGuard<LoopData<I,P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let packet_id = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
        let (o, c) = match data.one_time.entry(OneTimeKey::Subscribe(packet_id)) {
            Entry::Vacant(v) => {
                return Err(SourceError::Response(ErrorKind::from(
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
                let (tx, rx) = unbounded::<Result<SubItem>>();
                let custom_rx = rx.map_err(|_| {
                    Error::from(ErrorKind::LoopCommsError)
                });
                let filter = TopicFilter::from_string(&sub.topic).map_err(|e| {
                    SourceError::Response(e)
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

    fn process_unsub_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let pid = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
        let (o, c) = match data.one_time.entry(OneTimeKey::Unsubscribe(pid)) {
            Entry::Vacant(v) => {
                return Err(SourceError::Response(ErrorKind::from(
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

    fn process_ping_resp(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        if let Entry::Occupied(o) = data.one_time.entry(
            OneTimeKey::PingReq) {
            let (_, client) = o.remove();
            let _ = client.send(Ok(ClientReturn::Onetime(None)));
        }
        Ok(None)
    }

    fn process_pub(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        // Scream for the publish
        match packet.flags.qos() {
            QualityOfService::QoS0 => {
                let topic = match packet.headers.get("topic_name").unwrap() {
                    &Headers::TopicName(ref t) => t.clone(),
                    _ => unreachable!()
                };
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
                let id = match packet.headers.get("packet_id").unwrap() {
                    &Headers::PacketId(p) => p,
                    _ => unreachable!()
                };

                let topic = match packet.headers.get("topic_name").unwrap() {
                    &Headers::TopicName(ref t) => t.clone(),
                    _ => unreachable!()
                };
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
                Ok(Some(MqttPacket::pub_ack_packet(id)))
            },
            QualityOfService::QoS2 => {
                let id = match packet.headers.get("packet_id").unwrap() {
                    &Headers::PacketId(p) => p,
                    _ => unreachable!()
                };

                // Check if we have an existing publish with the same id
                if data.server_publish_state.contains_key(&id) {
                    return Err(SourceError::Response(ProtoErrorKind::QualityOfServiceError(
                        packet.flags.qos(),
                        format!("Duplicate publish recieved with same Packet ID: {}", id)
                    ).into()))
                } else {
                    data.server_publish_state.insert(id, PublishState::Received(packet));
                    // Send PUBREC
                    Ok(Some(MqttPacket::pub_rec_packet(id)))
                }
            }
        }
    }

    fn process_pub_ack(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let id = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
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
                        .map_err(|e| SourceError::Response(e))
                }
            }
            Ok(None)
        } else {
            bail!(SourceError::Response(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_rec(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let id = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Sent(p, s)) => (p, s),
                _ => unreachable!()
            };
            if let Err(e) = data.persistence.remove(p_key) {
                if let Some(s) = sender {
                    let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                }
                return Err(ErrorKind::PersistenceError.into())
                    .map_err(|e| SourceError::Response(e))
            }
            let rel = MqttPacket::pub_rel_packet(id);
            let ser = bincode::serialize(&rel, bincode::Infinite).unwrap();
            let key = match data.persistence.append(ser) {
                Ok(k) => k,
                Err(e) => {
                    if let Some(s) = sender {
                        let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                    }
                    return Err(ErrorKind::PersistenceError.into())
                        .map_err(|e| SourceError::Response(e))
                }
            };
            let _ = data.client_publish_state.insert(id, PublishState::Released(key, sender));

            Ok(Some(rel))
        } else {
            bail!(SourceError::Response(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_rel(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let id = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
        if data.server_publish_state.contains_key(&id) {
            let orig = match data.server_publish_state.remove(&id) {
                Some(PublishState::Received(o)) => o,
                _ => unreachable!()
            };

            let topic = match orig.headers.get("topic_name").unwrap() {
                &Headers::TopicName(ref t) => t.clone(),
                _ => unreachable!()
            };
            let payload = match orig.payload {
                Payload::Application(d) => Bytes::from(d),
                _ => unreachable!()
            };
            for &(ref filter, ref sender) in data.subscriptions.values() {
                if filter.match_topic(&topic) {
                    let _ = sender.send(Ok((topic.clone().into(), payload.clone())));
                }
            }

            Ok(Some(MqttPacket::pub_comp_packet(id)))
        } else {
            bail!(SourceError::Response(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }

    fn process_pub_comp(mut data: FutMutexGuard<LoopData<I, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError> {
        let id = match packet.headers.get("packet_id").unwrap() {
            &Headers::PacketId(p) => p,
            _ => unreachable!()
        };
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
                        .map_err(|e| SourceError::Response(e))
                }
            }
            Ok(None)
        } else {
            bail!(SourceError::Response(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone()).into()))
        }
    }
}

impl<I, P> Future for ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    type Item = SourceItem<I>;
    type Error = SourceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop { match self.state.take() {
            State::Reading => {
                let (packet, stream) = match self.future.poll() {
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Ok(Async::Ready((Some(r), s))) => (r, s),
                    Ok(Async::Ready((None, _))) => return Err(SourceError::Response(
                        ErrorKind::UnexpectedDisconnect.into())),
                    Err((e, _)) => {
                        return Err(SourceError::Response(ErrorKind::LoopCommsError.into()))
                    }
                };
                self.stream = Some(stream);
                self.state = Take::new(State::Processing(packet));
            },
            State::Processing(packet) => {
                // Get a lock on the data
                let data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Processing(packet));
                        return Ok(Async::NotReady)
                    }
                };
                packet.validate().map_err(|e| SourceError::Response(e))?;
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
                    p @ _ => return Err(SourceError::Response(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(p.ty.clone())
                    ).into()))
                };
                if let Some(s) = state {
                    self.state = Take::new(State::Sending(s));
                } else {
                    return Ok(Async::Ready(SourceItem::Response(self.stream.take().unwrap())));
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
                    Err(e) => return Err(SourceError::Request(e))
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
                    .map_err(|e| SourceError::Response(e)));
                return Ok(Async::Ready(SourceItem::Response(self.stream.take().unwrap())));
            }
        };}
    }
}
