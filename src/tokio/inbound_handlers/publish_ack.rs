use std::collections::hash_map::Entry;
use std::ops::Deref;

use ::bytes::Bytes;
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

pub fn publish_ack_handler<'p, P>(packet: MqttPacket, data_lock: FutMutex<LoopData<'p, P>>) ->
    Box<Future<Item=(), Error=Error> + 'p> where P: 'p + Persistence {

    Box::new(poll_fn(move || {
        let mut data = match data_lock.poll_lock() {
            Async::Ready(g) => g,
            Async::NotReady => return Ok(Async::NotReady)
        };

        let id = packet.headers.get::<PacketId>().unwrap();
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Sent(p, s)) => (p, s),
                _ => unreachable!()
            };
            match data.persistence.remove(p_key) {
                Ok(_) => {
                    if let Some(s) = sender {
                        let _ = s.send(Ok(ClientReturn::Onetime(None)));
                    }
                },
                Err(e) => {
                    if let Some(s) = sender {
                        let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                    }
                    return Err(ErrorKind::PersistenceError.into())
                }
            }
        } else {
            return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
        }

        Ok(Async::Ready(()))
    }).fuse())
}
