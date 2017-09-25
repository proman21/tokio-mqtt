use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Future};
use ::futures::sync::oneshot::Sender;
use ::take::Take;
use ::futures_mutex::FutMutex;
use ::bincode;

use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType, QualityOfService, PacketId};
use ::errors::{Error, Result, ErrorKind, ResultExt};
use ::tokio::mqtt_loop::LoopData;
use ::tokio::{
    OneTimeKey,
    PublishState,
    ClientReturn,
    RequestTuple
};

enum State {
    Processing(MqttPacket, Sender<Result<ClientReturn>>),
    Done
}

pub struct PingRequestHandler<'p, P> where P: 'p + Persistence {
    data_lock: FutMutex<LoopData<'p, P>>,
    state: Option<State>
}

impl<'p, P> PingRequestHandler<'p, P> where P: 'p + Persistence {
    pub fn new((packet, client): RequestTuple, data_lock: FutMutex<LoopData<'p, P>>) ->
        PingRequestHandler<'p, P> {

        PingRequestHandler {
            state: Some(State::Processing(packet, client)),
            data_lock
        }
    }
}

impl<'p, P> Future for PingRequestHandler<'p, P> where P: 'p + Persistence {
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

                data.one_time.insert(OneTimeKey::PingReq, (packet, client));
                self.state = Some(Done);
                Ok(Async::Ready(()))
            },
            Some(Done) => Ok(Async::NotReady),
            None => unreachable!()
        }
    }
}
