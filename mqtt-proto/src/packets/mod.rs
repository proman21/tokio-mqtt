pub mod connect;
pub mod connect_ack;
pub mod publish;
pub mod subscribe;
pub mod sub_ack;
pub mod unsubscribe;
pub mod ping;
pub mod disconnect;
pub mod util;
#[macro_use] mod contents;

mod prelude {
    pub use std::marker::PhantomData;
    pub use std::default::Default;
    pub use bytes::BufMut;
    pub use parsers::*;
    pub use types::*;
    pub use super::util::*;
    pub use super::{Headers, Payload, Contents};
    pub use crate::MqttPacket;
    pub use errors::*;
}

use self::prelude::*;
use super::MqttPacket;


pub trait Headers<'a>: Encodable + Sized {
    fn parse(input: &'a [u8], flags: &PacketFlags) -> IResult<&'a [u8], Self>;
}

pub trait Payload<'a, H: Headers<'a>>: Encodable + Sized {
    fn parse(input: &'a [u8], flags: &PacketFlags, headers: &H) -> IResult<&'a [u8], Self>;
}

pub enum Contents<'a> {
    Connect {
        headers: connect::ConnectHeaders<'a>,
        payload: connect::ConnectPayload<'a>
    },
    ConnectAck(connect_ack::ConnectAckHeaders),
    Publish {
        headers: publish::PublishHeaders<'a>,
        payload: publish::PublishPayload<'a>
    },
    PubAck(util::PacketIdHeaders),
    PubRec(util::PacketIdHeaders),
    PubRel(util::PacketIdHeaders),
    PubComp(util::PacketIdHeaders),
    Subscribe {
        headers: util::PacketIdHeaders,
        payload: subscribe::SubscribePayload<'a>
    },
    SubAck {
        headers: util::PacketIdHeaders,
        payload: sub_ack::SubAckPayload
    },
    Unsubscribe {
        headers: util::PacketIdHeaders,
        payload: unsubscribe::UnsubscribePayload<'a>
    },
    UnsubAck(util::PacketIdHeaders),
    PingReq,
    PingResp,
    Disconnect
}


impl<'a> MqttPacket<'a> {
    pub fn packet_type(&self) -> PacketType {
        self.contents.packet_type()
    }

    pub fn packet_flags(&self) -> PacketFlags {
        self.flags.clone()
    }

    pub fn contents(&'a self) -> &'a Contents<'a> {
        &self.contents
    }
    /// Attempts to decode a MQTT Control Packet from the provided slice of bytes.
    ///
    /// If there is a fully formed packet in the slice, a tuple will be returned that contains the
    /// packet and the rest of the slice.
    ///
    /// If there is not enough bytes in the slice to create a fully formed packet, `Ok(None)` will
    /// be returned.
    ///
    /// If an error occurs decoding the bytes, an error will be returned.
    pub fn from_slice(buf: &'a [u8]) -> Result<Option<(&'a [u8], MqttPacket<'a>)>> {
        let (rest, (ty, fl, re)) = fixed_header(buf).map_err(|e| Error::ParseFailure(e.into_error_kind()))?;

        if rest.len() < re {
            return Ok(None);
        }
        let (remain, after) = rest.split_at(re);
        let (_, contents) = Contents::parse(remain, &ty, &fl).map_err(|e| Error::ParseFailure(e.into_error_kind()))?;

        Ok(Some((after, MqttPacket {
            flags: fl,
            contents
        })))
    }

    pub fn encode<B: BufMut>(&self, out: &mut B) -> Result<()> {
        out.put_u8(((self.contents.packet_type() as u8) << 4) + self.flags.bits());

        self.contents.encode(out)?;

        Ok(())
    }

    pub fn len(&self) -> Option<usize> {
        self.contents.len().map(|s| s + 1)
    }
}
