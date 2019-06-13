use bytes::BufMut;
use enum_primitive::FromPrimitive;
use errors::*;
use std::convert::{AsRef, From, TryFrom};
use std::fmt;

static CRC_0_MESSAGE: &'static str = "0x00 Connection Accepted";
static CRC_1_MESSAGE: &'static str = "0x01 Connection Refused, unacceptable protocol version";
static CRC_2_MESSAGE: &'static str = "0x02 Connection Refused, identifier rejected";
static CRC_3_MESSAGE: &'static str = "0x03 Connection Refused, Server unavailable";
static CRC_4_MESSAGE: &'static str = "0x04 Connection Refused, bad user name or password";
static CRC_5_MESSAGE: &'static str = "0x05 Connection Refused, not authorized";

bitflags! {
    pub(crate) struct PacketFlags: u8 {
        const DUP  = 0b1000;
        const QOS2 = 0b0100;
        const QOS1 = 0b0010;
        const RET  = 0b0001;
    }
}

impl PacketFlags {
    pub fn qos(&self) -> QualityOfService {
        if self.intersects(Self::QOS2 | Self::QOS1) {
            if self.contains(Self::QOS2) {
                QualityOfService::QoS2
            } else {
                QualityOfService::QoS1
            }
        } else {
            QualityOfService::QoS0
        }
    }

    pub fn is_retain(&self) -> bool {
        self.intersects(Self::RET)
    }

    pub fn is_duplicate(&self) -> bool {
        self.intersects(Self::DUP)
    }
}

impl TryFrom<u8> for PacketFlags {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<PacketFlags, Error<'static>> {
        PacketFlags::from_bits(value).ok_or(Error::InvalidPacketFlag { flags: value })
    }
}

impl From<QualityOfService> for PacketFlags {
    fn from(value: QualityOfService) -> PacketFlags {
        match value {
            QualityOfService::QoS0 => PacketFlags::empty(),
            QualityOfService::QoS1 => PacketFlags::QOS1,
            QualityOfService::QoS2 => PacketFlags::QOS2,
        }
    }
}

bitflags! {
    pub(crate) struct ConnFlags: u8 {
        const USERNAME    = 0b10000000;
        const PASSWORD    = 0b01000000;
        const WILL_RETAIN = 0b00100000;
        const WILL_QOS2   = 0b00010000;
        const WILL_QOS1   = 0b00001000;
        const WILL_FLAG   = 0b00000100;
        const CLEAN_SESS  = 0b00000010;
    }
}

impl ConnFlags {
    pub fn is_clean(&self) -> bool {
        self.intersects(ConnFlags::CLEAN_SESS)
    }

    pub fn has_username(&self) -> bool {
        self.intersects(ConnFlags::USERNAME)
    }

    pub fn has_password(&self) -> bool {
        self.intersects(ConnFlags::PASSWORD)
    }

    pub fn lwt_retain(&self) -> bool {
        self.intersects(ConnFlags::WILL_RETAIN)
    }

    pub fn has_lwt(&self) -> bool {
        self.intersects(ConnFlags::WILL_FLAG)
    }
}

impl TryFrom<u8> for ConnFlags {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<ConnFlags, Error<'static>> {
        ConnFlags::from_bits(value).ok_or(Error::InvalidConnectFlags { flags: value })
    }
}

impl From<QualityOfService> for ConnFlags {
    fn from(value: QualityOfService) -> ConnFlags {
        match value {
            QualityOfService::QoS0 => ConnFlags::empty(),
            QualityOfService::QoS1 => ConnFlags::WILL_QOS1,
            QualityOfService::QoS2 => ConnFlags::WILL_QOS2,
        }
    }
}

bitflags! {
    pub struct ConnAckFlags: u8 {
        const SP = 0b0001;
    }
}

impl ConnAckFlags {
    pub fn is_clean(&self) -> bool {
        self.intersects(Self::SP)
    }
}

impl TryFrom<u8> for ConnAckFlags {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<ConnAckFlags, Error<'static>> {
        ConnAckFlags::from_bits(value).ok_or(Error::InvalidConnAckFlags { flags: value })
    }
}

pub enum PublishType {
    QoS0,
    QoS1 { packet_id: u16, dup: bool },
    QoS2 { packet_id: u16, dup: bool },
}

impl PublishType {
    pub(crate) fn new<'a>(
        flags: PacketFlags,
        packet_id: Option<u16>,
    ) -> Result<PublishType, Error<'a>> {
        match flags.qos() {
            QualityOfService::QoS0 => {
                ensure!(!flags.is_duplicate(), InvalidDupFlag);
                ensure!(packet_id.is_none(), UnexpectedPublishPacketId);
                Ok(PublishType::QoS0)
            }
            QualityOfService::QoS1 => {
                packet_id
                    .ok_or(Error::MissingPublishPacketId)
                    .map(|id| PublishType::QoS1 {
                        packet_id: id,
                        dup: flags.is_duplicate(),
                    })
            }
            QualityOfService::QoS2 => {
                packet_id
                    .ok_or(Error::MissingPublishPacketId)
                    .map(|id| PublishType::QoS2 {
                        packet_id: id,
                        dup: flags.is_duplicate(),
                    })
            }
        }
    }

    pub fn packet_id(&self) -> Option<u16> {
        match self {
            PublishType::QoS0 => None,
            PublishType::QoS1 { packet_id, .. } => Some(*packet_id),
            PublishType::QoS2 { packet_id, .. } => Some(*packet_id),
        }
    }

    pub(crate) fn packet_flags(&self) -> PacketFlags {
        match self {
            PublishType::QoS0 => PacketFlags::empty(),
            PublishType::QoS1 { dup, .. } => {
                let mut flags = PacketFlags::QOS1;
                flags.set(PacketFlags::DUP, *dup);
                flags
            }
            PublishType::QoS2 { dup, .. } => {
                let mut flags = PacketFlags::QOS2;
                flags.set(PacketFlags::DUP, *dup);
                flags
            }
        }
    }
}

impl From<PublishType> for QualityOfService {
    fn from(value: PublishType) -> QualityOfService {
        match value {
            PublishType::QoS0 => QualityOfService::QoS0,
            PublishType::QoS1 { .. } => QualityOfService::QoS1,
            PublishType::QoS2 { .. } => QualityOfService::QoS2,
        }
    }
}

enum_from_primitive! {
    /// Types of packets in the MQTT Protocol.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum PacketType {
        Connect     = 1,
        ConnAck     = 2,
        Publish     = 3,
        PubAck      = 4,
        PubRec      = 5,
        PubRel      = 6,
        PubComp     = 7,
        Subscribe   = 8,
        SubAck      = 9,
        Unsubscribe = 10,
        UnsubAck    = 11,
        PingReq     = 12,
        PingResp    = 13,
        Disconnect  = 14,
    }
}

impl TryFrom<u8> for PacketType {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<PacketType, Error<'static>> {
        PacketType::from_u8(value).ok_or(Error::UnknownPacketType { ty: value })
    }
}

impl fmt::Display for PacketType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::PacketType::*;
        match self {
            Connect => write!(f, "CONNECT"),
            ConnAck => write!(f, "CONN_ACK"),
            Publish => write!(f, "PUBLISH"),
            PubAck => write!(f, "PUBACK"),
            PubRec => write!(f, "PUBREC"),
            PubRel => write!(f, "PUBREL"),
            PubComp => write!(f, "PUBCOMP"),
            Subscribe => write!(f, "SUBSCRIBE"),
            SubAck => write!(f, "SUBACK"),
            Unsubscribe => write!(f, "UNSUBSCRIBE"),
            UnsubAck => write!(f, "UNSUBACK"),
            PingReq => write!(f, "PINGREQ"),
            PingResp => write!(f, "PINGRESP"),
            Disconnect => write!(f, "DISCONNECT"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnectError {
    BadProtoVersion = 1,
    ClientIdRejected = 2,
    ServerUnavailable = 3,
    BadCredentials = 4,
    Unauthorized = 5,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::ConnectError::*;

        match self {
            BadProtoVersion => write!(f, "{}", CRC_1_MESSAGE),
            ClientIdRejected => write!(f, "{}", CRC_2_MESSAGE),
            ServerUnavailable => write!(f, "{}", CRC_3_MESSAGE),
            BadCredentials => write!(f, "{}", CRC_4_MESSAGE),
            Unauthorized => write!(f, "{}", CRC_5_MESSAGE),
        }
    }
}

pub(crate) fn connect_result_from_u8(code: u8) -> Result<Result<(), ConnectError>, Error<'static>> {
    match code {
        0 => Ok(Ok(())),
        1 => Ok(Err(ConnectError::BadProtoVersion)),
        2 => Ok(Err(ConnectError::ClientIdRejected)),
        3 => Ok(Err(ConnectError::ServerUnavailable)),
        4 => Ok(Err(ConnectError::BadCredentials)),
        5 => Ok(Err(ConnectError::Unauthorized)),
        _ => Err(Error::InvalidConnectReturnCode { code }),
    }
}

enum_from_primitive! {
    /// The protocol version used by a connection.
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[allow(non_camel_case_types)]
    pub enum ProtoLvl {
        V3_1_1 = 4
    }
}

impl TryFrom<u8> for ProtoLvl {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<ProtoLvl, Error<'static>> {
        ProtoLvl::from_u8(value).ok_or(Error::InvalidProtocol { level: value })
    }
}

enum_from_primitive! {
    /// Set of quality of service levels a message can be sent with. These provide certain guarantees about the delivery
    /// of messages.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum QualityOfService {
        /// QoS Level 1: At most once delivery.
        /// The server will not respond to the message and the client will not attempt resending.
        QoS0 = 0,
        /// QoS Level 2: At least once delivery.
        /// The server will acknowledge the receiving of the message. Message might be sent more then once if an
        /// acknowledgement is not received in time.
        QoS1 = 1,
        /// QoS Level 3: Exactly once delivery.
        /// The client and server will both ensure the message is received by requiring a two-step acknowledgement that
        /// prevents loss or duplication.
        QoS2 = 2
    }
}

impl TryFrom<u8> for QualityOfService {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<QualityOfService, Error<'static>> {
        QualityOfService::from_u8(value).ok_or(Error::InvalidQos { qos: value })
    }
}

impl From<ConnFlags> for QualityOfService {
    fn from(value: ConnFlags) -> QualityOfService {
        if value.intersects(ConnFlags::WILL_QOS2) {
            QualityOfService::QoS2
        } else if value.intersects(ConnFlags::WILL_QOS1) {
            QualityOfService::QoS1
        } else {
            QualityOfService::QoS0
        }
    }
}

impl fmt::Display for QualityOfService {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &QualityOfService::QoS0 => write!(f, "QOS0"),
            &QualityOfService::QoS1 => write!(f, "QOS1"),
            &QualityOfService::QoS2 => write!(f, "QOS2"),
        }
    }
}

impl Encodable for QualityOfService {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u8(*self as u8)
    }

    fn encoded_length(&self) -> usize {
        1
    }
}

impl Encodable for Option<QualityOfService> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        let code = match self {
            Some(QualityOfService::QoS0) => 0,
            Some(QualityOfService::QoS1) => 1,
            Some(QualityOfService::QoS2) => 2,
            None => 128,
        };
        out.put_u8(code);
    }

    fn encoded_length(&self) -> usize {
        1
    }
}

pub(crate) trait Encodable {
    // Encodes the packet section into a buffer.
    fn encode<B: BufMut>(&self, out: &mut B);
    // Returns the size of the encoded section.
    fn encoded_length(&self) -> usize;
}

impl<T: Encodable> Encodable for Vec<T> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        for item in self {
            item.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        self.into_iter().fold(0, |acc, t| acc + t.encoded_length())
    }
}

impl Encodable for &[u8] {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u16_be(self.len() as u16);
        out.put_slice(self);
    }

    fn encoded_length(&self) -> usize {
        2 + self.len()
    }
}

/// A wrapper type around a `&str` that enforces the requirements for a MQTT packet string.
/// 1. The string length is <= 65535 bytes.
/// 2. Does not contain the null character U+0000.
#[derive(PartialEq, Debug, Clone)]
pub struct MqttString<'a>(&'a str);

impl<'a> MqttString<'a> {
    /// Create a new MqttString using `s`. Will return an error if the string requirements aren't met.
    pub fn new(s: &'a str) -> Result<MqttString<'a>, Error<'a>> {
        ensure!(s.len() <= 65535, StringTooBig { string: s });
        ensure!(!s.contains('\0'), InvalidString { string: s });

        Ok(MqttString(s))
    }

    /// Creates a new MqttString that is not checked for validity. Only use if you know the string is valid.
    pub fn new_unchecked(s: &'a str) -> MqttString<'a> {
        MqttString(s)
    }
}

impl<'a> AsRef<str> for MqttString<'a> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl<'a> fmt::Display for MqttString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'a> Encodable for MqttString<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u16_be(self.0.len() as u16);
        out.put_slice(self.0.as_bytes());
    }

    fn encoded_length(&self) -> usize {
        2 + self.as_ref().len()
    }
}

/// A tuple of the topic and requested Quality of Service level for a subscription request.
pub struct SubscriptionTuple<'a>(pub MqttString<'a>, pub QualityOfService);

impl<'a> Encodable for SubscriptionTuple<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.0.encode(out);
        self.1.encode(out);
    }

    fn encoded_length(&self) -> usize {
        self.0.encoded_length() + self.1.encoded_length()
    }
}

/// A Last Will and Testament message.
///
/// This type holds the Last Will and Testament message sent to the server upon connection.
/// If the client unexpectedly disconnects, this message will be sent by the server.
#[derive(Builder, Clone)]
pub struct LWTMessage<'a, P: AsRef<[u8]>> {
    pub topic: MqttString<'a>,
    #[builder(default = QualityOfService::QoS0)]
    pub qos: QualityOfService,
    #[builder(default = "false")]
    pub retain: bool,
    #[builder(default = [])]
    pub message: P,
}

impl<'a, P: AsRef<[u8]>> LWTMessage<'a, P> {
    pub(crate) fn from_flags(flags: ConnFlags, t: MqttString<'a>, m: P) -> LWTMessage<'a, P> {
        LWTMessage {
            topic: t,
            qos: flags.into(),
            retain: flags.intersects(ConnFlags::WILL_RETAIN),
            message: m,
        }
    }

    /// Converts `LWTMessage<P>` to `LWTMessage<&[u8]>`.
    pub fn as_ref(&self) -> LWTMessage<'a, &[u8]> {
        LWTMessage {
            topic: self.topic.clone(),
            qos: self.qos,
            retain: self.retain,
            message: self.message.as_ref(),
        }
    }

    pub(crate) fn connect_flags(&self) -> ConnFlags {
        let mut flags: ConnFlags = self.qos.into();
        flags.set(ConnFlags::WILL_RETAIN, self.retain);
        flags
    }
}

impl<'a> Encodable for LWTMessage<'a, &[u8]> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.topic.encode(out);
        self.message.encode(out);
    }

    fn encoded_length(&self) -> usize {
        self.topic.encoded_length() + self.message.encoded_length()
    }
}

/// Container for MQTT credentials, which is a username and optional password.
pub struct Credentials<'a, P: AsRef<[u8]>> {
    pub username: MqttString<'a>,
    pub password: Option<P>,
}

impl<'a, P: AsRef<[u8]>> Credentials<'a, P> {
    /// Converts `Credentials<P>` to `Credentials<&[u8]>`.
    pub fn as_ref(&self) -> Credentials<'a, &[u8]> {
        Credentials {
            username: self.username.clone(),
            password: self.password.as_ref().map(|p| p.as_ref()),
        }
    }
}

impl<'a> Encodable for Credentials<'a, &[u8]> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.username.encode(out);
        if let Some(pass) = self.password {
            pass.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        self.username.encoded_length() + self.password.map_or(0, |t| t.encoded_length())
    }
}
