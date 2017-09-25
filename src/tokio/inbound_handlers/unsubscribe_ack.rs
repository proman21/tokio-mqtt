use std::collections::hash_map::Entry;
use std::ops::Deref;

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
    BoxFuture
};

enum State {
    Processing(MqttPacket),
    Done
}

pub struct UnsubscribeAckHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> UnsubscribeAckHandler<'p, P> where P: 'p + Persistence {
    pub fn new(packet: MqttPacket, data_lock: FutMutex<LoopData<'p, P>>) ->
        UnsubscribeAckHandler<'p, P> {

        UnsubscribeAckHandler {
            state: Some(State::Processing(packet)),
            data_lock
        }
    }
}

impl<'p, P> Future for UnsubscribeAckHandler<'p, P> where P: 'p + Persistence {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::State::*;

        match self.state {
            Some(Processing(_)) => {
                let mut data = match self.data_lock.poll_lock() {
                    Async::Ready(g) => g,
                    Async::NotReady => return Ok(Async::NotReady)
                };

                let packet = match self.state.take() {
                    Some(Processing(packet)) => packet,
                    _ => unreachable!()
                };

                let pid = packet.headers.get::<PacketId>().unwrap();
                let (o, c) = match data.one_time.entry(OneTimeKey::Unsubscribe(*pid)) {
                    Entry::Vacant(v) => {
                        return Err(ErrorKind::from(ProtoErrorKind::UnexpectedResponse(
                            packet.ty.clone())).into());
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

                self.state = Some(Done);
                Ok(Async::Ready(()))
            },
            Some(Done) => Ok(Async::NotReady),
            None => unreachable!()
        }
    }
}
