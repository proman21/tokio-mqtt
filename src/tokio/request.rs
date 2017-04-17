use std::collections::hash_map::Entry;
use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Stream, Future};
use ::futures::stream::StreamFuture;
use ::futures::sync::mpsc::UnboundedReceiver;
use ::futures::sync::oneshot::Sender;
use ::take::Take;
use ::futures_mutex::FutMutex;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType, Headers, QualityOfService, Payload};
use ::errors::{Result, Error, ErrorKind, ResultExt};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::types::SubItem;
use super::MqttFramedReader;
use super::mqtt_loop::LoopData;
use super::{SourceItem, SourceError, ClientReturn, OneTimeKey, TopicFilter, PublishState, ClientQueue};

enum State {
    Reading,
    Processing((MqttPacket, Sender<Result<ClientReturn>>)),
    Writing
}

pub struct RequestProcessor<I, P> where I: AsyncRead + AsyncWrite, P: Persistence {
    state: Take<State>,
    data: FutMutex<LoopData<I, P>>,
    future: StreamFuture<ClientQueue>,
    stream: Option<ClientQueue>
}

impl RequestProcessor<I, P> where I: AsyncRead + AsyncWrite, P: Persistence {
    pub fn new(future: StreamFuture<ClientQueue>, data: FutMutex<LoopData<I, P>>) -> RequestProcessor<I, P> {
        RequestProcessor {
            state: Take::new(State::Reading),
            data: data,
            future: future,
            stream: None
        }
    }
}

impl<I, P> Future for RequestProcessor<I, P> where I: AsyncRead + AsyncWrite, P: Persistence {
    type Item = SourceItem;
    type Error = SourceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop { match self.state.take() {
            State::Reading => {
                let (packet, stream) = match self.future.poll() {
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Ok(Async::Ready((Some(r), s))) => (r, s),
                    Ok(Async::Ready((None, _))) => return Err(SourceError::Request(
                        ErrorKind::UnexpectedDisconnect.into())),
                    Err((e, _)) => return Err(SourceError::Request(e))
                };
                self.stream = Some(stream);
                self.state = Take::new(State::Processing(packet));
            },
            State::Processing((packet, client)) => {
                // Get a lock on the data
                let data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Processing(packet));
                        return Ok(Async::NotReady)
                    }
                };
                self.state = match packet {
                    p @ MqttPacket{ty: PacketType::Connect, ..} => {
                        data.one_time.insert(OneTimeKey::Connect, (p.clone(), client));

                        loop {
                            match data.framed_write.start_send(p.clone()) {
                                Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                Ok(AsyncSink::NotReady(_)) => continue,
                                Err(e) => return Err(SourceError::Request(e))
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Subscribe, ..} => {
                        let id = match packet.headers.get("packet_id").unwrap() {
                            &Headers::PacketId(p) => p,
                            _ => unreachable!()
                        };
                        data.one_time.insert(OneTimeKey::Subscribe(id), (p.clone(), client));

                        loop {
                            match data.framed_write.start_send(p.clone()) {
                                Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                Ok(AsyncSink::NotReady(_)) => continue,
                                Err(e) => return Err(SourceError::Request(e))
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Unsubscribe, ..} => {
                        let id = match p.headers.get("packet_id").unwrap() {
                            &Headers::PacketId(p) => p,
                            _ => unreachable!()
                        };
                        data.one_time.insert(OneTimeKey::Unsubscribe(id), (p.clone(), client));

                        loop {
                            match data.framed_write.start_send(p.clone()) {
                                Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                Ok(AsyncSink::NotReady(_)) => continue,
                                Err(e) => return Err(SourceError::Request(e))
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::PingReq, ..} => {
                        data.one_time.insert(OneTimeKey::PingReq, (p.clone(), client));

                        loop {
                            match data.framed_write.start_send(p.clone()) {
                                Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                Ok(AsyncSink::NotReady(_)) => continue,
                                Err(e) => return Err(SourceError::Request(e))
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Publish, ..} => {
                        match p.flags.qos() {
                            QualityOfService::QoS0 => {
                                loop {
                                    match data.framed_write.start_send(p.clone()) {
                                        Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                        Ok(AsyncSink::NotReady(_)) => continue,
                                        Err(e) => return Err(SourceError::Request(e))
                                    }
                                }
                            },
                            QualityOfService::QoS1 | QualityOfService::QoS2 => {
                                let id = match p.headers.get("packet_id").unwrap() {
                                    &Headers::PacketId(p) => p,
                                    _ => unreachable!()
                                };

                                let ser = bincode::serialize(&p, bincode::Infinite).unwrap();
                                let key = match data.persistence.append(ser) {
                                    Ok(k) => k,
                                    Err(e) => {
                                        if let Some(s) = sender {
                                            let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                                        }
                                        return Err(ErrorKind::PersistenceError.into())
                                            .map_err(|e| SourceError::Request(e))
                                    }
                                };
                                data.client_publish_state.insert(id,
                                    PublishState::Sent(key, Some(client)));

                                loop {
                                    match data.framed_write.start_send(p.clone()) {
                                        Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                        Ok(AsyncSink::NotReady(_)) => continue,
                                        Err(e) => return Err(SourceError::Request(e))
                                    }
                                }
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Disconnect, ..} => {
                        loop {
                            match data.framed_write.start_send(p.clone()) {
                                Ok(AsyncSink::Ready) => return Ok(State::Writing),
                                Ok(AsyncSink::NotReady(_)) => continue,
                                Err(e) => return Err(SourceError::Request(e))
                            }
                        }
                    },
                    p @ _ => unreachable!()
                };
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
                    .map_err(|e| SourceError::Request(e)));
                return Ok(Async::Ready(SourceItem::Request(self.stream.take().unwrap())));
            }
        };}
    }
}
