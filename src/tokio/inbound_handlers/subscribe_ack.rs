use std::collections::hash_map::Entry;

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

pub struct SubscribeAckHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> SubscribeAckHandler<'p, P> where P: 'p + Persistence {
    pub fn new(packet: MqttPacket, data_lock: FutMutex<LoopData<'p, P>>) ->
        SubscribeAckHandler<'p, P> {

        SubscribeAckHandler {
            state: Some(State::Processing(packet)),
            data_lock
        }
    }
}

impl<'p, P> Future for SubscribeAckHandler<'p, P> where P: 'p + Persistence {
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

                let packet_id = packet.headers.get::<PacketId>().unwrap();
                let (o, c) = match data.one_time.entry(OneTimeKey::Subscribe(*packet_id)) {
                    Entry::Vacant(v) => {
                        self.state = Some(Done);
                        return Err(ErrorKind::from(ProtoErrorKind::UnexpectedResponse(
                            packet.ty.clone())).into());
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
                        let (tx, rx) = unbounded::<SubItem>();
                        let custom_rx = rx.map_err(|_| {
                            Error::from(ErrorKind::LoopCommsError)
                        });
                        let filter = TopicFilter::from_string(&sub.topic)?;
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

                self.state = Some(Done);
                Ok(Async::Ready(()))
            },
            Some(Done) => Ok(Async::NotReady),
            None => unreachable!()
        }
    }
}
