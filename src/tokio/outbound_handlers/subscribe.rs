use ::futures::{Poll, Async, Future};
use ::futures::sync::oneshot::Sender;
use ::futures_mutex::FutMutex;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketId};
use ::errors::{Error, Result};
use ::tokio::mqtt_loop::LoopData;
use ::tokio::{OneTimeKey, ClientReturn, RequestTuple};

enum State {
    Processing(MqttPacket, Sender<Result<ClientReturn>>),
    Done
}

pub struct SubscribeHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> SubscribeHandler<'p, P> where P: 'p + Persistence {
    pub fn new((packet, client): RequestTuple, data_lock: FutMutex<LoopData<'p, P>>) ->
        SubscribeHandler<'p, P> {

        SubscribeHandler {
            state: Some(State::Processing(packet, client)),
            data_lock
        }
    }
}

impl<'p, P> Future for SubscribeHandler<'p, P> where P: 'p + Persistence {
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

                let (packet, client) = match self.state.take() {
                    Some(Processing(packet, client)) => (packet, client),
                    _ => unreachable!()
                };

                let id = packet.headers.get::<PacketId>().unwrap();
                data.one_time.insert(OneTimeKey::Subscribe(*id), (packet, client));
                self.state = Some(Done);
                Ok(Async::Ready(()))
            },
            Some(Done) => Ok(Async::NotReady),
            None => unreachable!()
        }
    }
}
