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

#[cfg(test)]
mod encodable_tests {
    use super::{Encodable, QualityOfService};

    #[test]
    fn vec_encodable() {
        let data = vec![QualityOfService::QoS0, QualityOfService::QoS1, QualityOfService::QoS2];
        let mut buf: Vec<u8> = Vec::new();
        data.encode(&mut buf);
        assert_eq!(data.encoded_length(), 3);
        assert_eq!(buf, vec![0, 1, 2]);
    }

    #[test]
    fn slice_encodable() {
        let data = [1, 34, 96, 39, 20];
        let mut buf: Vec<u8> = Vec::new();
        (&data[..]).encode(&mut buf);
        assert_eq!((&data[..]).encoded_length(), 7);
        assert_eq!(buf, vec![0, 5, 1, 34, 96, 39, 20]);
    }
}

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
        if self.intersects(Self::QOS2) {
            QualityOfService::QoS2
        } else if self.intersects(Self::QOS1) {
            QualityOfService::QoS1
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
        let flags = PacketFlags::from_bits(value).ok_or(Error::InvalidPacketFlag { flags: value })?;
        ensure!(!flags.contains(PacketFlags::QOS1 | PacketFlags::QOS2), InvalidPacketFlag { flags: value });
        Ok(flags)
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

#[cfg(test)]
mod packet_flag_tests {
    use super::{Error, PacketFlags, QualityOfService};
    use std::convert::TryFrom;

    #[test]
    fn conversion() {
        // Basic conversion
        assert_eq!(PacketFlags::try_from(0b0001), Ok(PacketFlags::RET));
        assert_eq!(PacketFlags::try_from(0b0010), Ok(PacketFlags::QOS1));
        assert_eq!(PacketFlags::try_from(0b0100), Ok(PacketFlags::QOS2));
        assert_eq!(PacketFlags::try_from(0b1000), Ok(PacketFlags::DUP));
        assert_eq!(PacketFlags::try_from(0b0110), Err(Error::InvalidPacketFlag{ flags: 0b0110 }));

        // Multiple flags
        assert_eq!(PacketFlags::try_from(0b1010), Ok(PacketFlags::DUP | PacketFlags::QOS1));
        assert_eq!(PacketFlags::try_from(0b0101), Ok(PacketFlags::QOS2 | PacketFlags::RET));
    }

    #[test]
    fn qos() {
        assert_eq!(PacketFlags::QOS1.qos(), QualityOfService::QoS1);
        assert_eq!(PacketFlags::QOS2.qos(), QualityOfService::QoS2);
        assert_eq!(PacketFlags::RET.qos(), QualityOfService::QoS0);
    }

    #[test]
    fn is_retain() {
        assert!(PacketFlags::RET.is_retain());
        assert!(!PacketFlags::QOS1.is_retain());
        assert!(!PacketFlags::QOS2.is_retain());
        assert!(!PacketFlags::DUP.is_retain());
    }

    #[test]
    fn is_duplicate() {
        assert!(PacketFlags::DUP.is_duplicate());
        assert!(!PacketFlags::RET.is_duplicate());
        assert!(!PacketFlags::QOS1.is_duplicate());
        assert!(!PacketFlags::QOS2.is_duplicate());
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
    pub fn lwt_qos<'a>(&self) -> QualityOfService {
        if self.intersects(ConnFlags::WILL_QOS1) {
            QualityOfService::QoS1
        } else if self.intersects(ConnFlags::WILL_QOS2) {
            QualityOfService::QoS2
        } else {
            QualityOfService::QoS0
        }
    }

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
        let flags = ConnFlags::from_bits(value).ok_or(Error::InvalidConnectFlags { flags: value })?;
        
        if flags.has_password() {
            ensure!(flags.has_username(), InvalidConnectFlags{ flags: value });
        }
        if flags.has_lwt() {
            ensure!(!flags.contains(ConnFlags::WILL_QOS1 | ConnFlags::WILL_QOS2),
                InvalidConnectFlags { flags: value });
        } else {
            ensure!(!flags.lwt_retain(), InvalidConnectFlags{ flags: value });
            ensure!(flags.lwt_qos() == QualityOfService::QoS0, InvalidConnectFlags{ flags: value });
        }

        Ok(flags)
    }
}

impl From<QualityOfService> for ConnFlags {
    fn from(value: QualityOfService) -> ConnFlags {
        match value {
            QualityOfService::QoS0 => ConnFlags::WILL_FLAG,
            QualityOfService::QoS1 => ConnFlags::WILL_QOS1 | ConnFlags::WILL_FLAG,
            QualityOfService::QoS2 => ConnFlags::WILL_QOS2 | ConnFlags::WILL_FLAG,
        }
    }
}

#[cfg(test)]
mod conn_flags_tests {
    use super::{ConnFlags, Error, QualityOfService};
    use std::convert::TryFrom;

    #[test]
    fn conversion() {
        assert_eq!(ConnFlags::try_from(0b10000000), Ok(ConnFlags::USERNAME));
        assert_eq!(ConnFlags::try_from(0b11000000), Ok(ConnFlags::USERNAME | ConnFlags::PASSWORD));
        assert_eq!(ConnFlags::try_from(0b00100100), Ok(ConnFlags::WILL_RETAIN | ConnFlags::WILL_FLAG));
        assert_eq!(ConnFlags::try_from(0b00010100), Ok(ConnFlags::WILL_QOS2 | ConnFlags::WILL_FLAG));
        assert_eq!(ConnFlags::try_from(0b00001100), Ok(ConnFlags::WILL_QOS1 | ConnFlags::WILL_FLAG));
        assert_eq!(ConnFlags::try_from(0b00000100), Ok(ConnFlags::WILL_FLAG));
        assert_eq!(ConnFlags::try_from(0b00000010), Ok(ConnFlags::CLEAN_SESS));
        assert_eq!(ConnFlags::try_from(0b00000000), Ok(ConnFlags::empty()));

        assert_eq!(ConnFlags::try_from(0b00000001), Err(Error::InvalidConnectFlags{ flags: 1 }));
        assert_eq!(ConnFlags::try_from(0b01000000), Err(Error::InvalidConnectFlags{ flags: 0b01000000 }));
        assert_eq!(ConnFlags::try_from(0b00100000), Err(Error::InvalidConnectFlags{ flags: 0b00100000 }));
        assert_eq!(ConnFlags::try_from(0b00010000), Err(Error::InvalidConnectFlags{ flags: 0b00010000 }));
        assert_eq!(ConnFlags::try_from(0b00001000), Err(Error::InvalidConnectFlags{ flags: 0b00001000 }));
        assert_eq!(ConnFlags::try_from(0b00011100), Err(Error::InvalidConnectFlags{ flags: 0b00011100 }));
    }

    #[test]
    fn lwt_qos() {
        assert_eq!(ConnFlags::WILL_QOS1.lwt_qos(), QualityOfService::QoS1);
        assert_eq!(ConnFlags::WILL_QOS2.lwt_qos(), QualityOfService::QoS2);
        assert_eq!(ConnFlags::empty().lwt_qos(), QualityOfService::QoS0);
    }

    #[test]
    fn is_clean() {
        assert!(ConnFlags::CLEAN_SESS.is_clean());
        assert!(!(!ConnFlags::CLEAN_SESS).is_clean());
    }

    #[test]
    fn has_username() {
        assert!(ConnFlags::USERNAME.has_username());
        assert!(!(!ConnFlags::USERNAME).has_username());
    }

    #[test]
    fn has_password() {
        assert!(ConnFlags::PASSWORD.has_password());
        assert!(!(!ConnFlags::PASSWORD).has_password());
    }

    #[test]
    fn has_lwt() {
        assert!(ConnFlags::WILL_FLAG.has_lwt());
        assert!(!(!ConnFlags::WILL_FLAG).has_lwt());
    }

    #[test]
    fn lwt_retain() {
        assert!(ConnFlags::WILL_RETAIN.lwt_retain());
        assert!(!(!ConnFlags::WILL_RETAIN).lwt_retain());
    }

    #[test]
    fn from_qos() {
        assert_eq!(ConnFlags::from(QualityOfService::QoS0), ConnFlags::WILL_FLAG);
        assert_eq!(ConnFlags::from(QualityOfService::QoS1), ConnFlags::WILL_FLAG | ConnFlags::WILL_QOS1);
        assert_eq!(ConnFlags::from(QualityOfService::QoS2), ConnFlags::WILL_FLAG | ConnFlags::WILL_QOS2);
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

#[derive(Clone, Debug, PartialEq)]
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

    /// Get the packet ID, if one exists
    /// 
    /// # Examples
    /// ```
    /// # use mqtt3_proto::PublishType;
    /// assert_eq!(PublishType::QoS0.packet_id(), None);
    /// assert_eq!(PublishType::QoS1{ packet_id: 1, dup: false }.packet_id(), Some(1));
    /// assert_eq!(PublishType::QoS2{ packet_id: 1, dup: false }.packet_id(), Some(1));
    /// ```
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

#[cfg(test)]
mod publish_type_tests {
    use super::{PublishType, PacketFlags, Error};

    #[test]
    fn new() {
        assert_eq!(PublishType::new(PacketFlags::empty(), None),
            Ok(PublishType::QoS0));
        assert_eq!(PublishType::new(PacketFlags::RET, None),
            Ok(PublishType::QoS0));
        assert_eq!(PublishType::new(PacketFlags::DUP, None), Err(Error::InvalidDupFlag));
        assert_eq!(PublishType::new(PacketFlags::RET, Some(20)), Err(Error::UnexpectedPublishPacketId));
        
        assert_eq!(PublishType::new(PacketFlags::QOS1, Some(20)),
            Ok(PublishType::QoS1{ packet_id: 20, dup: false}));
        assert_eq!(PublishType::new(PacketFlags::QOS1 | PacketFlags::DUP, Some(20)),
            Ok(PublishType::QoS1{ packet_id: 20, dup: true}));
        assert_eq!(PublishType::new(PacketFlags::QOS1, None), Err(Error::MissingPublishPacketId));
        
        assert_eq!(PublishType::new(PacketFlags::QOS2, Some(20)),
            Ok(PublishType::QoS2{ packet_id: 20, dup: false}));
        assert_eq!(PublishType::new(PacketFlags::QOS2 | PacketFlags::DUP, Some(20)),
            Ok(PublishType::QoS2{ packet_id: 20, dup: true}));
        assert_eq!(PublishType::new(PacketFlags::QOS2, None), Err(Error::MissingPublishPacketId));
    }

    #[test]
    fn packet_flags() {
        assert_eq!(PublishType::QoS0.packet_flags(), PacketFlags::empty());
        assert_eq!(PublishType::QoS1{packet_id: 10, dup: false}.packet_flags(), PacketFlags::QOS1);
        assert_eq!(PublishType::QoS1{packet_id: 10, dup: true}.packet_flags(), PacketFlags::QOS1 | PacketFlags::DUP);
        assert_eq!(PublishType::QoS2{packet_id: 10, dup: false}.packet_flags(), PacketFlags::QOS2);
        assert_eq!(PublishType::QoS2{packet_id: 10, dup: true}.packet_flags(), PacketFlags::QOS2 | PacketFlags::DUP);
    }
}

enum_from_primitive! {
    /// Types of packets in the MQTT Protocol.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, PartialEq, Debug)]
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

#[cfg(test)]
mod connect_error_tests {
    use super::{connect_result_from_u8, ConnectError, Error};

    #[test]
    fn conversion() {
        assert_eq!(connect_result_from_u8(0), Ok(Ok(())));
        assert_eq!(connect_result_from_u8(1), Ok(Err(ConnectError::BadProtoVersion)));
        assert_eq!(connect_result_from_u8(2), Ok(Err(ConnectError::ClientIdRejected)));
        assert_eq!(connect_result_from_u8(3), Ok(Err(ConnectError::ServerUnavailable)));
        assert_eq!(connect_result_from_u8(4), Ok(Err(ConnectError::BadCredentials)));
        assert_eq!(connect_result_from_u8(5), Ok(Err(ConnectError::Unauthorized)));
        assert_eq!(connect_result_from_u8(128), Err(Error::InvalidConnectReturnCode { code: 128 }));
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
    /// Return code for a SubAck packet, indicating either the maximum quality of service of the subscription,
    /// or a failure to subscribe.
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[repr(u8)]
    pub enum SubAckReturnCode {
        /// Subscription was successfully acknowledged with a maximum QoS 0.
        QoS0 = 0,
        /// Subscription was successfully created with a maximum QoS 1.
        QoS1 = 1,
        /// Subscription was successfully created with a maximum QoS 2.
        QoS2 = 2,
        /// Server failed to create subscription.
        Failure = 0x80,
    }
}

impl TryFrom<u8> for SubAckReturnCode {
    type Error = Error<'static>;

    fn try_from(value: u8) -> Result<SubAckReturnCode, Error<'static>> {
        SubAckReturnCode::from_u8(value).ok_or(Error::InvalidSubAckReturnCode { code: value })
    }
}

impl fmt::Display for SubAckReturnCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &SubAckReturnCode::QoS0 => write!(f, "Success - Maximum QoS 0"),
            &SubAckReturnCode::QoS1 => write!(f, "Success - Maximum QoS 1"),
            &SubAckReturnCode::QoS2 => write!(f, "Success - Maximum QoS 2"),
            &SubAckReturnCode::Failure => write!(f, "Failure"),
        }
    }
}

impl Encodable for SubAckReturnCode {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u8(*self as u8)
    }

    fn encoded_length(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod sub_ack_return_code_tests {
    use super::{Encodable, SubAckReturnCode};

    #[test]
    fn sub_ack_return_code_encode() {
        let mut buf: Vec<u8> = Vec::new();
        SubAckReturnCode::QoS0.encode(&mut buf);
        assert_eq!(buf, vec![0]);
        buf.clear();
        SubAckReturnCode::QoS1.encode(&mut buf);
        assert_eq!(buf, vec![1]);
        buf.clear();
        SubAckReturnCode::QoS2.encode(&mut buf);
        assert_eq!(buf, vec![2]);
        buf.clear();
        SubAckReturnCode::Failure.encode(&mut buf);
        assert_eq!(buf, vec![128])
    }
}

enum_from_primitive! {
    /// Set of quality of service levels a message can be sent with. These provide certain guarantees about the delivery
    /// of messages.
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[repr(u8)]
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

#[cfg(test)]
mod qos_tests {
    use super::{Encodable, QualityOfService};

    #[test]
    fn qos_encode() {
        let mut buf: Vec<u8> = Vec::new();
        QualityOfService::QoS0.encode(&mut buf);
        assert_eq!(buf, vec![0]);
        buf.clear();
        QualityOfService::QoS1.encode(&mut buf);
        assert_eq!(buf, vec![1]);
        buf.clear();
        QualityOfService::QoS2.encode(&mut buf);
        assert_eq!(buf, vec![2]);
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

impl<'a> PartialEq<str> for MqttString<'a> {
    fn eq(&self, other: &str) -> bool {
        self.0.eq(other)
    }
}

impl<'a> TryFrom<&'a str> for MqttString<'a> {
    type Error = Error<'a>;

    fn try_from(value: &'a str) -> Result<MqttString<'a>, Error<'a>> {
        MqttString::new(value)
    }
}

impl<'a> From<MqttString<'a>> for &'a str {
    fn from(value: MqttString<'a>) -> &'a str {
        value.0
    }
}

impl<'a> Encodable for MqttString<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.0.as_bytes().encode(out);
    }

    fn encoded_length(&self) -> usize {
        self.0.as_bytes().encoded_length()
    }
}

#[cfg(test)]
mod mqtt_string_tests {
    use super::{MqttString, Error};

    #[test]
    fn new() {
        assert_eq!(MqttString::new("test"), Ok(MqttString("test")));
        assert_eq!(MqttString::new("has a null\0"), Err(Error::InvalidString{ string: "has a null\0"}));
    }
}

/// A tuple of the topic and requested Quality of Service level for a subscription request.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Builder, Clone, Debug, PartialEq)]
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
            qos: flags.lwt_qos(),
            retain: flags.lwt_retain(),
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
#[derive(Clone, Debug, PartialEq)]
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
