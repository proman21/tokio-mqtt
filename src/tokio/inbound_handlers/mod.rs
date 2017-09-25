mod subscribe_ack;
mod unsubscribe_ack;
mod ping_response;
mod publish;
mod publish_ack;
mod publish_recieve;
mod publish_release;
mod publish_complete;

use ::futures::{Poll, Future};
use ::futures::unsync::mpsc::UnboundedSender;
use ::futures_mutex::FutMutex;
use ::errors::Error;
use ::proto::MqttPacket;
use ::tokio::LoopRequest;
use ::tokio::mqtt_loop::LoopData;
use ::persistence::Persistence;

pub struct ResponseHandler<'p> {
    inner: Box<Future<Item=(), Error=Error> + 'p>
}

impl<'p> ResponseHandler<'p> {
    pub fn new<P>(packet: MqttPacket, requester: UnboundedSender<LoopRequest>,
        data_lock: FutMutex<LoopData<'p, P>>) -> ResponseHandler<'p> where P: 'p + Persistence {

        use ::proto::PacketType::*;

        let inner = match packet.ty {
            PingResp => ping_response::ping_response_handler(data_lock),
            PubAck => publish_ack::publish_ack_handler(packet, data_lock),
            _ => unreachable!()
        };

        ResponseHandler {
            inner: inner
        }
    }
}

impl<'p> Future for ResponseHandler<'p> {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<(), Error> {
        self.inner.poll()
    }
}
