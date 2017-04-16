use std::sync::{Arc, Mutex, TryLockError};
use std::collections::hash_map::Entry;
use std::ops::Deref;
use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async};
use ::futures::future::{Future};
use ::futures::stream::{Stream, StreamFuture};
use ::futures::sync::mpsc::{unbounded};
use ::take::Take;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType, Headers, QualityOfService, Payload};
use ::errors::{Result, Error, ErrorKind};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::types::{BoxMqttStream, SubItem};
use super::MqttFramedReader;
use super::mqtt_loop::LoopData;
use super::{SourceItem, SourceError, ClientReturn, OneTimeKey};

enum State<I> {
    Reading,
    Locking(MqttPacket, MqttFramedReader<I>)
}

pub struct ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    state: Take<State<I>>,
    data: Arc<Mutex<LoopData<I, P>>>,
    future: StreamFuture<MqttFramedReader<I>>
}

impl<I, P> ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    pub fn new(future: StreamFuture<MqttFramedReader<I>>, data: Arc<Mutex<LoopData<I, P>>>) -> ResponseProcessor<I, P> {
        ResponseProcessor {
            state: Take::new(State::Reading),
            data: data,
            future: future
        }
    }
}

impl<I, P> Future for ResponseProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence{
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
                    Err((e, _)) => return Err(SourceError::Response(e))
                };
                self.state = Take::new(State::Locking(packet, stream));
            },
            State::Locking(packet, stream) => {
                // Get a lock on the data
                let mut data = match self.data.try_lock() {
                    Ok(g) => g,
                    Err(TryLockError::WouldBlock) => return Ok(Async::NotReady),
                    Err(TryLockError::Poisoned(_)) => return Err(SourceError::Response(
                        ErrorKind::LoopAbortError.into()))
                };
                match packet {
                    p @ MqttPacket{ty: PacketType::ConnAck, ..} => {
                        // Check if we are awaiting a CONNACK
                        let (_, client) = match data.one_time.entry(OneTimeKey::Connect) {
                            Entry::Vacant(v) => {
                                return Err(SourceError::Response(ErrorKind::from(
                                        ProtoErrorKind::UnexpectedResponse(p.ty.clone())
                                ).into()));
                            },
                            Entry::Occupied(o) => o.remove()
                        };
                        // Validate connect return code
                        let crc = match p.headers.get("connect_return_code").unwrap() {
                            &Headers::ConnRetCode(crc) => crc,
                            _ => unreachable!()
                        };
                        if crc.is_ok() {
                            // If session is not clean, setup a stream.
                            let sess = match p.headers.get("connect_ack_flags").unwrap() {
                                &Headers::ConnAckFlags(fl) => fl,
                                _ => unreachable!()
                            };
                            if sess.is_clean() {
                                let _ = client.send(Ok(ClientReturn::Onetime(Some(p))));
                            } else {
                                let (tx, rx) = unbounded::<Result<SubItem>>();
                                let custom_rx = rx.map_err(|_| {
                                    Error::from(ErrorKind::LoopCommsError)
                                });
                                data.session_subs = Some(tx);
                                let _ = client.send(Ok(ClientReturn::Ongoing(
                                    vec![Ok((custom_rx.boxed(),QualityOfService::QoS0))]
                                )));
                            }
                        } else {
                            let _ = client.send(Err(ErrorKind::from(
                                ProtoErrorKind::ConnectionRefused(crc)).into()));
                            return Err(SourceError::Response(ErrorKind::from(
                                ProtoErrorKind::ConnectionRefused(crc.clone())).into()
                            ))
                        }
                    },
                    p @ MqttPacket{ty: PacketType::SubAck, ..} => {
                        let packet_id = match p.headers.get("packet_id").unwrap() {
                            &Headers::PacketId(p) => p,
                            _ => unreachable!()
                        };
                        let (o, c) = match data.one_time.entry(OneTimeKey::Subscribe(packet_id)) {
                            Entry::Vacant(v) => {
                                return Err(SourceError::Response(ErrorKind::from(
                                    ProtoErrorKind::UnexpectedResponse(p.ty.clone())).into()));
                            },
                            Entry::Occupied(o) => o.remove()
                        };
                        // Validate each return code
                        let orig = match o.payload {
                            Payload::Subscribe(v) => v,
                            _ => unreachable!()
                        };
                        let ret_codes = match p.payload {
                            Payload::SubAck(v) => v,
                            _ => unreachable!()
                        };
                        let mut collect: Vec<Result<(BoxMqttStream<Result<SubItem>>, QualityOfService)>> = Vec::new();
                        for (sub, ret) in orig.iter().zip(ret_codes.iter()) {
                            if ret.is_ok() {
                                let (tx, rx) = unbounded::<Result<SubItem>>();
                                let custom_rx = rx.map_err(|_| {
                                    Error::from(ErrorKind::LoopCommsError)
                                });
                                data.subscriptions.insert(sub.topic.clone().into_inner(), tx);
                                collect.push(Ok((custom_rx.boxed(), sub.qos)));
                            } else {
                                collect.push(Err(ErrorKind::from(
                                    ProtoErrorKind::SubscriptionRejected(
                                        sub.topic.clone().into_inner(), sub.qos)
                                    ).into()
                                ));
                            }
                        }
                        let _ = c.send(Ok(ClientReturn::Ongoing(collect)));
                    },
                    p @ MqttPacket{ty: PacketType::UnsubAck, ..} => {
                        let pid = match p.headers.get("packet_id").unwrap() {
                            &Headers::PacketId(p) => p,
                            _ => unreachable!()
                        };
                        let (o, c) = match data.one_time.entry(OneTimeKey::Unsubscribe(pid)) {
                            Entry::Vacant(v) => {
                                return Err(SourceError::Response(ErrorKind::from(
                                    ProtoErrorKind::UnexpectedResponse(p.ty.clone())).into()));
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
                    },
                    p @ MqttPacket{ty: PacketType::PingResp, ..} => {
                        if let Entry::Occupied(o) = data.one_time.entry(
                            OneTimeKey::PingReq) {
                            let (_, client) = o.remove();
                            let _ = client.send(Ok(ClientReturn::Onetime(None)));
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Publish, ..} => {
                        // Scream for the publish
                        match p.flags.qos() {
                            QualityOfService::QoS0 => {
                                let topic = match p.headers.get("topic_name").unwrap() {
                                    &Headers::TopicName(ref t) => t.clone(),
                                    _ => unreachable!()
                                };

                            },
                            QualityOfService::QoS1 => unimplemented!(),
                            QualityOfService::QoS2 => unimplemented!()
                        }
                    },
                    p @ _ => return Err(SourceError::Response(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(p.ty.clone())
                    ).into()))
                }
                return Ok(Async::Ready(SourceItem::Response(stream)));
            }
        };}
    }
}
