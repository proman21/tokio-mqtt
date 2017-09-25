use ::futures::{Async, Future};
use ::futures::future::poll_fn;
use ::futures_mutex::FutMutex;

use ::proto::{MqttPacket, PacketId};
use ::persistence::Persistence;
use ::errors::{Error, ErrorKind, ResultExt};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::tokio::mqtt_loop::LoopData;
use ::tokio::{PublishState, ClientReturn};

pub fn publish_complete_handler<'p, P>(packet: MqttPacket, data_lock: FutMutex<LoopData<'p, P>>) ->
    Box<Future<Item=(), Error=Error> + 'p> where P: 'p + Persistence {

    Box::new(poll_fn(move || {
        let mut data = match data_lock.poll_lock() {
            Async::Ready(g) => g,
            Async::NotReady => return Ok(Async::NotReady)
        };

        let id = packet.headers.get::<PacketId>().unwrap();
        if data.client_publish_state.contains_key(&id) {
            let (p_key, sender) = match data.client_publish_state.remove(&id) {
                Some(PublishState::Released(p, s)) => (p, s),
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
