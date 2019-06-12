use snafu::Snafu;
use std::str::Utf8Error;
use types::{MqttString, PacketType};

#[derive(Snafu, Debug, Clone, PartialEq)]
#[snafu(visibility(pub(crate)))]
pub enum Error<'a> {
    #[snafu(display("Packet size exceeded maximum size. Encoded size is {}.", encoded_size))]
    PacketTooBig { encoded_size: usize },
    #[snafu(display("'{}' excedes maximum size for a MQTT UTF-8 string.", string))]
    StringTooBig { string: &'a str },
    #[snafu(display("Input '{:?}' is not a valid UTF-8 string: {}", input, source))]
    StringNotUtf8 { input: &'a [u8], source: Utf8Error },
    #[snafu(display("'{}' contains Unicode chars not allowed in MQTT strings.", string))]
    InvalidString { string: &'a str },
    #[snafu(display("{} is not a known MQTT packet type.", ty))]
    UnknownPacketType { ty: u8 },
    #[snafu(display("Unrecognised protocol level '{}'", level))]
    InvalidProtocol { level: u8 },
    #[snafu(display("Protocol name must be 'MQTT', got {}.", name))]
    InvalidProtocolName { name: MqttString<'a> },
    #[snafu(display("{:b} is not a set of valid packet flags.", flags))]
    InvalidPacketFlag { flags: u8 },
    #[snafu(display("Publish DUP flag must be 0 for QoS0."))]
    InvalidDupFlag,
    #[snafu(display("Publish packet @ QoS1/QoS2 needs a packet id."))]
    MissingPublishPacketId,
    #[snafu(display("Publish packet @ QoS0 must not have a packet id."))]
    UnexpectedPublishPacketId,
    #[snafu(display("Packet payload cannot be empty."))]
    MissingPayload,
    #[snafu(display("Remaining packet length is larger then actual remaining packet."))]
    IncompletePacket,
    #[snafu(display("'{}' is not a valid return code.", return_code))]
    InvalidSubAckReturnCode { return_code: u8 },
    #[snafu(display("Session present should not be set if the connect return code is an error."))]
    UnexpectedSessionPresent,
    #[snafu(display("{:b} is not a valid set of Connect flags.", flags))]
    InvalidConnectFlags { flags: u8 },
    #[snafu(display("{:b} is not a valid set of ConnAck flags.", flags))]
    InvalidConnAckFlags { flags: u8 },
    #[snafu(display("Invalid connect return code '{}'.", code))]
    InvalidConnectReturnCode { code: u8 },
    #[snafu(display("VLE overflows allowed encoding size."))]
    VleOverflow,
    #[snafu(display("'{}' is not a valid Quality of Service level.", qos))]
    InvalidQos { qos: u8 },
    #[snafu(display("Expected {:b} for {} packet flags, got {:b}.", expected, ty, received))]
    UnexpectedPacketFlags {
        expected: u8,
        ty: PacketType,
        received: u8,
    },
}
