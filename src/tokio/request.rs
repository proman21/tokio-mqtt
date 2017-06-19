use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Future};
use ::futures::sync::oneshot::Sender;
use ::take::Take;
use ::futures_mutex::FutMutex;
use ::bincode;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType, Headers, QualityOfService, PacketId};
use ::errors::{Result, ErrorKind, ResultExt};
use super::mqtt_loop::LoopData;
use super::{
    SourceItem,
    SourceError,
    OneTimeKey,
    PublishState,
    ClientQueue,
    ClientReturn
};

enum State {
    Processing((MqttPacket, Sender<Result<ClientReturn>>)),
    Sending(MqttPacket),
    Writing
}

pub struct RequestProcessor<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence {
    state: Take<State>,
    data: FutMutex<LoopData<I, P>>,
    stream: Option<ClientQueue>
}

impl<I, P> RequestProcessor<I, P> where I: AsyncRead + AsyncWrite + Send, P: Persistence {
    pub fn new(req: (MqttPacket, Sender<Result<ClientReturn>>), data: FutMutex<LoopData<I, P>>,
        stream: ClientQueue) -> RequestProcessor<I, P> {
        RequestProcessor {
            state: Take::new(State::Processing(req)),
            data: data,
            stream: Some(stream)
        }
    }
}

impl<I, P> Future for RequestProcessor<I, P> where I: AsyncRead + AsyncWrite + Send, P: Persistence {
    type Item = SourceItem<I>;
    type Error = SourceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop { match self.state.take() {
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
                                            .map_err(|e| SourceError::Request(e))
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

                self.state = Take::new(State::Sending(packet))
            },
            State::Sending(packet) => {
                let mut data = match self.data.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => {
                        self.state = Take::new(State::Sending(packet));
                        return Ok(Async::NotReady)
                    }
                };
                let c = packet.clone();
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
                    .map_err(|e| SourceError::Request(e)));
                return Ok(Async::Ready(SourceItem::Request(self.stream.take().unwrap(), None)));
            }
        };}
    }
}
