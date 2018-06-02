mod codec;
//noinspection ALL
mod mqtt_loop;
mod ping;
mod outbound_handlers;
mod inbound_handlers;
mod response;
mod disconnect;
mod connect;

/// This imports some common items used to implement the various handlers of the loop actor.
mod prelude {
    pub use tokio_io::{AsyncRead, AsyncWrite};
    pub use futures::prelude::*;

    pub use proto::MqttPacket;
    pub use super::mqtt_loop::{Loop, LoopStatus};
    pub use errors::{Error, ErrorKind, Result};
    pub use errors::proto::ErrorKind as ProtoErrorKind;
    pub use persistence::Persistence;
}

pub use self::mqtt_loop::Loop;
pub use self::codec::MqttCodec;
pub use self::connect::Connect;

use ::futures::prelude::*;
use ::futures::{unsync, sync};
use ::futures::sync::mpsc::{UnboundedSender};
use ::futures::sync::oneshot::Sender;

use ::errors::{Result, ErrorKind, ResultExt};
use ::proto::{MqttPacket, QualityOfService};
use ::types::{Subscription, SubItem};
use ::persistence::Persistence;

type BoxFuture<T, E> = Box<Future<Item = T,Error = E>>;

type SubscriptionSender = UnboundedSender<Result<SubItem>>;
type ClientReturnSender<T> = Sender<Result<T>>;
type ClientReturnReceiver = Receiver<Result<ClientReturn>>;

enum LoopMpscSender<T> {
    Local(unsync::mpsc::UnboundedSender<T>),
    Remote(sync::mpsc::UnboundedSender<T>)
}

impl<T> LoopMpscSender<T> {
    pub fn local(s: unsync::mpsc::UnboundedSender<T>) -> LoopMpscSender<T> {
        LoopMpscSender::Local(s)
    }

    pub fn remote(s: sync::mpsc::UnboundedSender<T>) -> LoopMpscSender<T> {
        LoopMpscSender::Remote(s)
    }

    pub fn unbounded_send(&self, msg: T) -> Result<(), T> {
        use self::LoopMpscSender::*;
        match self {
            Local(ref s) => s.unbounded_send(msg).map_err(|e| e.into_inner()),
            Remote(ref s) => s.unbounded_send(msg).map_err(|e| e.into_inner())
        }
    }
}

pub enum LoopMpscReceiver<T> {
    Local(unsync::mpsc::UnboundedReceiver<T>),
    Remote(sync::mpsc::UnboundedReceiver<T>)
}

impl<T> LoopMpscReceiver<T> {
    pub fn local(r: unsync::mpsc::UnboundedReceiver<T>) -> LoopMpscReceiver<T> {
        LoopMpscReceiver::Local(r)
    }

    pub fn remote(r: sync::mpsc::UnboundedReceiver<T>) -> LoopMpscReceiver<T> {
        LoopMpscReceiver::Remote(r)
    }

    pub fn close(&mut self) {
        use self::LoopMpscReceiver::*;
        match self {
            Local(ref mut r) => r.close(),
            Remote(ref mut r) => r.close()
        }
    }
}

impl<T> Stream for LoopMpscReceiver<T> {
    type Item = T;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::LoopMpscReceiver::*;
        match self {
            Local(ref mut r) => r.poll(),
            Remote(ref mut r) => r.poll()
        }
    }
}

pub enum TimeoutRequest {
    Start(TimeoutType, Duration),
    Drop(TimeoutType)
}

pub enum TimeoutType {
    Connect,
    Ping,
    Disconnect
}

pub enum LoopMessage {
    Connect {
        ret: ClientReturnSender<Option<MqttStream<SubItem>>>,
        lwt: LWTMessage,
        cid: String,
        cred: Credentials<String>
    },
    Publish {
        ret: ClientReturnSender<()>,
        topic: String,
        qos: QualityOfService,
        retain: bool,
        msg: Bytes
    },
    Subscribe {
        ret: ClientReturnSender<Vec<Result<Subscription>>>,
        subs: Vec<(String, QualityOfService)>
    },
    Unsubscribe {
        ret: ClientReturnSender<()>,
        subs: Vec<String>
    },
    Disconnect {
        ret: ClientReturnSender<bool>,
        timeout: Option<u64>
    },
    Connection {
        packet: Result<MqttPacket>
    },
    Timeout {
        ty: TimeoutType
    }
}

/// # Quality of Service Flows
/// ## QoS1
/// ### Server-sent publish
/// 1. Receive publish
/// 2. Send acknowledgement
/// ### Client-sent publish
/// 1. Send packet, start at Sent
/// 2. Receive acknowledgement
/// ## QoS2
/// ### Server-sent publish
/// 1. Recieve message
/// 2. Send Received message, transition to Received
/// 3. Receive Release message
/// 4. Send Complete message.
/// ### Client-sent publish
/// 1. Send publish, start at Sent
/// 2. Receive Received message
/// 3. Send Release message, transition to Released.
/// 4. Receive Complete message
pub enum PublishState {
    Sent(String, Option<ClientReturnSender<>>),
    Received(MqttPacket),
    Released(String, Option<ClientReturnSender<>>)
}