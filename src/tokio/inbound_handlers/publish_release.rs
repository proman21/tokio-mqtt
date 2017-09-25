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

pub struct PublishReleaseHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> PublishReleaseHandler<'p, P> where P: 'p + Persistence {
    pub fn new(packet: MqttPacket, requester: UnboundedSender<LoopRequest>,
        data_lock: FutMutex<LoopData<'p, P>>) -> PublishReleaseHandler<'p, P> {

        PublishReleaseHandler {
            state: Some(State::Processing(packet, requester)),
            data_lock
        }
    }
}

impl<'p, P> Future for PublishReleaseHandler<'p, P> where P: 'p + Persistence {
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
                            let _ = sender.send((topic.clone().into(), payload.clone()));
                        }
                    }

                    requester.send(LoopRequest::Internal(MqttPacket::pub_comp_packet(*id)));
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
