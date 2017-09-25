use std::collections::hash_map::Entry;
use std::ops::Deref;

use ::bytes::Bytes;
use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Future, Stream};
use ::futures::sync::mpsc::unbounded;
use ::futures::unsync::mpsc::UnboundedSender;
use ::take::Take;
use ::futures_mutex::FutMutex;
use ::bincode;

use ::proto::{
    MqttPacket,
    PacketType,
    QualityOfService,
    Payload,
    ConnectReturnCode,
    ConnectAckFlags,
    PacketId,
    TopicName
};
use ::persistence::Persistence;
use ::errors::{Error, Result, ErrorKind, ResultExt};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::tokio::mqtt_loop::LoopData;
use ::tokio::{
    OneTimeKey,
    PublishState,
    ClientReturn,
    RequestTuple,
    TopicFilter,
    SubItem,
    BoxFuture,
    LoopRequest
};

enum State {
    Processing(MqttPacket, UnboundedSender<LoopRequest>),
    Done
}

pub struct PublishRecieveHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> PublishRecieveHandler<'p, P> where P: 'p + Persistence {
    pub fn new(packet: MqttPacket, requester: UnboundedSender<LoopRequest>,
        data_lock: FutMutex<LoopData<'p, P>>) -> PublishRecieveHandler<'p, P> {

        PublishRecieveHandler {
            state: Some(State::Processing(packet, requester)),
            data_lock
        }
    }
}

impl<'p, P> Future for PublishRecieveHandler<'p, P> where P: 'p + Persistence {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::State::*;

        match self.state {
            Some(Processing(_, _)) => {
                let mut data = match self.data_lock.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => return Ok(Async::NotReady)
                };

                let (packet, requester) = match self.state.take() {
                    Some(Processing(packet, requester)) => (packet, requester),
                    _ => unreachable!()
                };

                let id = packet.headers.get::<PacketId>().unwrap();
                if data.client_publish_state.contains_key(&id) {
                    let (p_key, sender) = match data.client_publish_state.remove(&id) {
                        Some(PublishState::Sent(p, s)) => (p, s),
                        _ => unreachable!()
                    };
                    if let Err(e) = data.persistence.remove(p_key) {
                        return Err(e).chain_err(|| ErrorKind::PersistenceError)
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
                        }
                    };
                    let _ = data.client_publish_state.insert(*id, PublishState::Released(key, sender));

                    requester.send(LoopRequest::Internal(rel));
                } else {
                    return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
                }

                self.state = Some(Done);
                Ok(Async::Ready(()))
            },
            Some(Done) => Ok(Async::NotReady),
            None => unreachable!()
        }
    }
}
