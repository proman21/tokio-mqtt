use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Future};
use ::futures::sync::oneshot::Sender;
use ::take::Take;
use ::futures_mutex::FutMutex;
use ::bincode;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType, QualityOfService, PacketId};
use ::errors::{Error, Result, ErrorKind, ResultExt};
use super::mqtt_loop::LoopData;
use super::{
    SourceItem,
    SourceError,
    OneTimeKey,
    PublishState,
    ClientReturn
};

enum State {
    Processing((MqttPacket, Sender<Result<ClientReturn>>)),
    Sending((MqttPacket, Sender<Result<ClientReturn>>)),
    Writing
}

pub struct RequestProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite + 'static, P: 'p + Persistence, 'p: 'static {
    state: Take<State>,
    data: FutMutex<LoopData<'p, I, P>>
}

impl<'p, I, P> RequestProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite, P: 'p + Persistence, 'p: 'static {
    pub fn new(req: (MqttPacket, Sender<Result<ClientReturn>>), data: FutMutex<LoopData<'p, I, P>>) -> RequestProcessor<'p, I, P> {
        RequestProcessor {
            state: Take::new(State::Sending(req)),
            data: data
        }
    }
}

impl<'p, I, P> Future for RequestProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite, P: 'p + Persistence, 'p: 'static {
    type Item = SourceItem<I>;
    type Error = SourceError<I>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop { match self.state.take() {
            State::Sending((packet, client)) => {
                let mut data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Sending((packet, client)));
                        return Ok(Async::NotReady)
                    }
                };
                let work = !(data.server_publish_state.is_empty() &&
                    data.client_publish_state.is_empty());
                let c = packet.clone();
                self.state = Take::new(match data.framed_write.start_send(c) {
                    Ok(AsyncSink::Ready) => State::Processing((packet, client)),
                    Ok(AsyncSink::NotReady(_)) => State::Sending((packet, client)),
                    Err(e) => {
                        match e {
                            Error(ErrorKind::PacketEncodingError, _) => {
                                let _ = client.send(Err(e));
                                return Ok(Async::Ready(SourceItem::ProcessRequest(false, work)));
                            },
                            _ => return Err(SourceError::ProcessRequest(e))
                        }
                    }
                })
            },
            State::Processing((packet, client)) => {
                // Get a lock on the data
                let mut data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Processing((packet, client)));
                        return Ok(Async::NotReady)
                    }
                };

                match packet {
                    p @ MqttPacket{ty: PacketType::Connect, ..} => {
                        data.one_time.insert(OneTimeKey::Connect, (p.clone(), client));
                    },
                    p @ MqttPacket{ty: PacketType::Subscribe, ..} => {
                        let id = p.headers.get::<PacketId>().unwrap();
                        data.one_time.insert(OneTimeKey::Subscribe(*id), (p.clone(), client));
                    },
                    p @ MqttPacket{ty: PacketType::Unsubscribe, ..} => {
                        let id = p.headers.get::<PacketId>().unwrap().clone();
                        data.one_time.insert(OneTimeKey::Unsubscribe(*id), (p.clone(), client));
                    },
                    p @ MqttPacket{ty: PacketType::PingReq, ..} => {
                        data.one_time.insert(OneTimeKey::PingReq, (p.clone(), client));
                    },
                    p @ MqttPacket{ty: PacketType::Publish, ..} => {
                        match p.flags.qos() {
                            QualityOfService::QoS0 => {},
                            QualityOfService::QoS1 | QualityOfService::QoS2 => {
                                let id = p.headers.get::<PacketId>().unwrap();

                                let ser = bincode::serialize(&p, bincode::Infinite).unwrap();
                                let key = match data.persistence.append(ser) {
                                    Ok(k) => k,
                                    Err(e) => {
                                        return Err(e).chain_err(|| ErrorKind::PersistenceError)
                                            .map_err(|e| SourceError::ProcessRequest(e))
                                    }
                                };
                                data.client_publish_state.insert(*id,
                                    PublishState::Sent(key, Some(client)));
                            }
                        }
                    },
                    p @ MqttPacket{ty: PacketType::Disconnect, ..} => {},
                    p @ _ => unreachable!()
                };

                self.state = Take::new(State::Writing)
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
                    .map_err(|e| SourceError::ProcessRequest(e)));
                let work = !(data.server_publish_state.is_empty() &&
                    data.client_publish_state.is_empty());
                return Ok(Async::Ready(SourceItem::ProcessRequest(true, work)));
            }
        };}
    }
}
