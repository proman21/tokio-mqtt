mod subscribe;
mod unsubscribe;
mod ping_request;
mod publish;

use futures::{Future, Poll};
use futures_mutex::FutMutex;
use ::tokio::{RequestTuple, BoxFuture};
use ::tokio::mqtt_loop::LoopData;
use ::persistence::Persistence;
use ::errors::Error;

pub struct RequestHandler<'p> {
    inner: Box<Future<Item=(), Error=Error> + 'p>
}

impl<'p> RequestHandler<'p> {
    pub fn new<P>((packet, client): RequestTuple, data_lock: FutMutex<LoopData<'p, P>>) ->
        RequestHandler where P: 'p + Persistence {

        use ::proto::PacketType::*;

        let inner: Box<Future<Item=(), Error=Error> + 'p> = match packet.ty {
            PingReq => Box::new(ping_request::PingRequestHandler::new((packet, client), data_lock)),
            Publish => Box::new(publish::PublishHandler::new((packet, client), data_lock)),
            Subscribe => Box::new(subscribe::SubscribeHandler::new((packet, client), data_lock)),
            Unsubscribe => Box::new(unsubscribe::UnsubscribeHandler::new((packet, client), data_lock)),
            _ => unreachable!()
        };

        RequestHandler {
            inner: inner
        }
    }
}

impl<'p> Future for RequestHandler<'p> {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
    }
}
