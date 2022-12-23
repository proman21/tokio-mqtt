mod codec;
mod reactor;
// mod ping;
// mod outbound_handlers;
// mod inbound_handlers;
// mod disconnect;
// mod connect;
pub(crate) mod channel;
mod state;

/// This imports some common items used to implement the various handlers of the loop.
mod prelude {
    pub use tokio::io::{AsyncRead, AsyncWrite};
    pub use futures::prelude::*;

    pub use mqtt3_proto::MqttPacket;
    pub use super::reactor::ReactorClient;
    pub use crate::errors::Error;
    pub use crate::persistence::Persistence;
}

pub use self::reactor::{Reactor, ReactorClient};
pub use self::codec::MqttCodec;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::config::Config;
use mqtt3_proto::{MqttPacket, PacketType};
use self::channel::{LoopRetSender, Subscription};
use self::codec::MqttPacketBuf;

pub enum ReactorCommand<'a> {
    Connect {
        io_read: &'a mut dyn AsyncRead,
        io_write: &'a mut dyn AsyncWrite,
        config: &'a Config<'a>,
        ret: LoopRetSender<'a, Subscription<'a>>
    },
    SendPacket {
        packet: MqttPacket<'a>,
        ret: LoopRetSender<'a, ()>
    },
    AwaitResponse {
        filter: PacketFilter,
        ret: LoopRetSender<'a, MqttPacketBuf<'a>>,
    },
    Subscribe {
        filter: &'a str,
        ret: LoopRetSender<'a, Subscription<'a>>,
    },
    Unsubscribe {
        filter: &'a str,
        ret: LoopRetSender<'a, ()>,
    },
    Disconnect {
        timeout: Option<u64>,
        ret: LoopRetSender<'a, bool>
    }
}

#[derive(Hash, PartialEq, Eq)]
pub enum PacketFilter {
    Ty(PacketType),
    Id((PacketType, u16)),
}

impl From<PacketType> for PacketFilter {
    fn from(value: PacketType) -> PacketFilter {
        PacketFilter::Ty(value)
    }
}

impl From<(PacketType, u16)> for PacketFilter {
    fn from(value: (PacketType, u16)) -> PacketFilter {
        PacketFilter::Id(value)
    }
}