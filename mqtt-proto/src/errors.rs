use std::result::Result as StdResult;
use std::str::Utf8Error;
use failure::Fail;
use nom::ErrorKind;

pub type Result<T> = StdResult<T, Error>;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "Packet size excedded maximum size. Encoded size is {}.", _0)]
    PacketTooBig(usize),
    #[fail(display = "{} is too big for a MQTT UTF-8 string.", _0)]
    StringTooBig(usize),
    #[fail(display = "String is not a valid UTF-8 string. {}", _0)]
    StringNotUtf8(#[cause] Utf8Error),
    #[fail(display = "Packet type {} is invalid", _0)]
    InvalidPacketType(u8),
    #[fail(display = "Error occurred while parsing packet: {:?}.", _0)]
    ParseFailure(ErrorKind<u32>),
    #[fail(display = "DUP flag must be 0 for QoS0.")]
    InvalidDupFlag,
    #[fail(display = "Publish packet @ QoS1/QoS2 needs a packet id.")]
    MissingPublishPacketId,
    #[fail(display = "Publish packet @ QoS0 must not have a packet id.")]
    UnexpectedPublishPacketId
}
