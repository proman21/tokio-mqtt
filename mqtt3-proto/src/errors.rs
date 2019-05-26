use std::result::Result as StdResult;
use std::str::Utf8Error;
use failure::Fail;
use nom::ErrorKind;
use types::PacketType;

pub type Result<T> = StdResult<T, Error>;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "Packet size exceeded maximum size. Encoded size is {}.", _0)]
    PacketTooBig(usize),
    #[fail(display = "{} is too big for a MQTT UTF-8 string.", _0)]
    StringTooBig(usize),
    #[fail(display = "String is not a valid UTF-8 string. {}", _0)]
    StringNotUtf8(#[cause] Utf8Error),
    #[fail(display = "{} is not a known MQTT packet type.", _0)]
    UnknownPacketType(u8),
    #[fail(display = "Unrecognised protocol level '{}'", _0)]
    InvalidProtocol(u8),
    #[fail(display = "Error occurred while parsing {} packet.", _0)]
    ParseFailure(PacketType),
    #[fail(display = "Publish DUP flag must be 0 for QoS0.")]
    InvalidDupFlag,
    #[fail(display = "Publish packet @ QoS1/QoS2 needs a packet id.")]
    MissingPublishPacketId,
    #[fail(display = "Publish packet @ QoS0 must not have a packet id.")]
    UnexpectedPublishPacketId,
    #[fail(display = "Packet payload cannot be empty.")]
    MissingPayload,
    #[fail(display = "'{}' is not a valid return code.", _0)]
    InvalidSubAckReturnCode(u8),
    #[fail(display = "Session present should not be set if the connect return code is an error.")]
    UnexpectedSessionPresent,
}
