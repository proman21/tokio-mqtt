use std::collections::hash_map::Entry;
use std::ops::Deref;

use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, AsyncSink, Sink, Future, Stream};
use ::futures::future::poll_fn;
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

pub fn ping_response_handler<'p, P>(data_lock: FutMutex<LoopData<'p, P>>)
    -> Box<Future<Item=(), Error=Error> + 'p> where P: 'p + Persistence {

    Box::new(poll_fn(move || {
        let mut data = match data_lock.poll_lock() {
            Async::Ready(g) => g,
            Async::NotReady => return Ok(Async::NotReady)
        };

        if let Entry::Occupied(o) = data.one_time.entry(
            OneTimeKey::PingReq) {
            let (_, client) = o.remove();
            let _ = client.send(Ok(ClientReturn::Onetime(None)));
        }

        Ok(Async::Ready(()))
    }).fuse())
}
