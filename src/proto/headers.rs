use std::ops::Deref;
use std::str;
use ::bytes::{Bytes, BytesMut, IntoBuf, Buf, BufMut, BigEndian};
use ::enum_primitive::FromPrimitive;
use super::types::*;
use super::Header;
use ::errors::{Result, ErrorKind, ResultExt};

pub struct ConnectFlags(ConnFlags);

impl ConnectFlags {
    pub fn new(flags: ConnFlags) -> ConnectFlags {
        ConnectFlags(flags)
    }
}

impl Deref for ConnectFlags {
    type Target = ConnFlags;

    fn deref(&self) -> &ConnFlags {
        &self.0
    }
}

impl Header for ConnectFlags {
    fn header_name() -> &'static str {
        "connect_flags"
    }

    fn parse_header(raw: &Bytes) -> Result<ConnectFlags> {
        ConnFlags::from_bits(raw.into_buf().get_u8()).map(|f| ConnectFlags(f))
            .ok_or(ErrorKind::PacketDecodingError.into())
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u8(self.bits());
        Ok(())
    }
}

pub struct ConnectAckFlags(ConnAckFlags);

impl ConnectAckFlags {
    pub fn new(flags: ConnAckFlags) -> ConnectAckFlags {
        ConnectAckFlags(flags)
    }
}

impl Deref for ConnectAckFlags {
    type Target = ConnAckFlags;

    fn deref(&self) -> &ConnAckFlags {
        &self.0
    }
}

impl Header for ConnectAckFlags {
    fn header_name() -> &'static str {
        "connect_ack_flags"
    }

    fn parse_header(raw: &Bytes) -> Result<ConnectAckFlags> {
        ConnAckFlags::from_bits(raw.into_buf().get_u8()).map(|f| ConnectAckFlags(f))
            .ok_or(ErrorKind::PacketDecodingError.into())
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u8(self.bits());
        Ok(())
    }
}

pub struct ConnectReturnCode(ConnRetCode);

impl ConnectReturnCode {
    pub fn new(code: ConnRetCode) -> ConnectReturnCode {
        ConnectReturnCode(code)
    }
}

impl Deref for ConnectReturnCode {
    type Target = ConnRetCode;

    fn deref(&self) -> &ConnRetCode {
        &self.0
    }
}

impl Header for ConnectReturnCode {
    fn header_name() -> &'static str {
        "connect_return_code"
    }

    fn parse_header(raw: &Bytes) -> Result<ConnectReturnCode> {
        ConnRetCode::from_u8(raw.into_buf().get_u8()).map(|c| ConnectReturnCode(c))
            .ok_or(ErrorKind::PacketDecodingError.into())
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u8(self.as_u8());
        Ok(())
    }
}

#[derive(Clone)]
pub struct PacketId(u16);

impl PacketId {
    pub fn new(id: u16) -> PacketId {
        PacketId(id)
    }
}

impl Deref for PacketId {
    type Target = u16;

    fn deref(&self) -> &u16 {
        &self.0
    }
}

impl Header for PacketId {
    fn header_name() -> &'static str {
        "packet_id"
    }

    fn parse_header(raw: &Bytes) -> Result<PacketId> {
        if raw.len() < 2 {
            bail!(ErrorKind::PacketDecodingError)
        } else {
            Ok(PacketId(raw.into_buf().get_u16::<BigEndian>()))
        }
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u16::<BigEndian>(self.0);
        Ok(())
    }
}

pub struct KeepAlive(u16);

impl KeepAlive {
    pub fn new(ka: u16) -> KeepAlive {
        KeepAlive(ka)
    }
}

impl Deref for KeepAlive {
    type Target = u16;

    fn deref(&self) -> &u16 {
        &self.0
    }
}

impl Header for KeepAlive {
    fn header_name() -> &'static str {
        "keep_alive"
    }

    fn parse_header(raw: &Bytes) -> Result<KeepAlive> {
        if raw.len() < 2 {
            bail!(ErrorKind::PacketDecodingError)
        } else {
            Ok(KeepAlive(raw.into_buf().get_u16::<BigEndian>()))
        }
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u16::<BigEndian>(self.0);
        Ok(())
    }
}

pub struct ProtocolLevel(ProtoLvl);

impl ProtocolLevel {
    pub fn new(level: ProtoLvl) -> ProtocolLevel {
        ProtocolLevel(level)
    }
}

impl Deref for ProtocolLevel {
    type Target = ProtoLvl;

    fn deref(&self) -> &ProtoLvl {
        &self.0
    }
}

impl Header for ProtocolLevel {
    fn header_name() -> &'static str {
        "protocol_level"
    }

    fn parse_header(raw: &Bytes) -> Result<ProtocolLevel> {
        ProtoLvl::from_u8(raw.into_buf().get_u8()).map(|c| ProtocolLevel(c))
            .ok_or(ErrorKind::PacketDecodingError.into())
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        out.put_u8(self.as_u8());
        Ok(())
    }
}

pub struct ProtocolName(MqttString);

impl ProtocolName {
    pub fn new(name: MqttString) -> ProtocolName {
        ProtocolName(name)
    }
}

impl Deref for ProtocolName {
    type Target = MqttString;

    fn deref(&self) -> &MqttString {
        &self.0
    }
}

impl Header for ProtocolName {
    fn header_name() -> &'static str {
        "protocol_name"
    }

    fn parse_header(raw: &Bytes) -> Result<ProtocolName> {
        str::from_utf8(raw)
            .chain_err(|| ErrorKind::StringConversionError)
            .and_then(|s| MqttString::from_str(s))
            .map(|s| ProtocolName(s))
            .chain_err(|| ErrorKind::PacketDecodingError)
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        self.encode(out);
        Ok(())
    }
}

pub struct TopicName(MqttString);

impl TopicName {
    pub fn new(name: MqttString) -> TopicName {
        TopicName(name)
    }
}

impl Deref for TopicName {
    type Target = MqttString;

    fn deref(&self) -> &MqttString {
        &self.0
    }
}

impl Header for TopicName {
    fn header_name() -> &'static str {
        "topic_name"
    }

    fn parse_header(raw: &Bytes) -> Result<TopicName> {
        str::from_utf8(raw)
            .chain_err(|| ErrorKind::StringConversionError)
            .and_then(|t| MqttString::from_str(t))
            .map(|s| TopicName(s))
            .chain_err(|| ErrorKind::PacketDecodingError)
    }

    fn fmt_header(&self, out: &mut BytesMut) -> Result<()> {
        self.encode(out);
        Ok(())
    }
}
