use std::fmt;
use ::bytes::{BufMut};

static CRC_0_MESSAGE: &'static str = "0x00 Connection Accepted";
static CRC_1_MESSAGE: &'static str = "0x01 Connection Refused, unacceptable protocol version";
static CRC_2_MESSAGE: &'static str = "0x02 Connection Refused, identifier rejected";
static CRC_3_MESSAGE: &'static str = "0x03 Connection Refused, Server unavailable";
static CRC_4_MESSAGE: &'static str = "0x04 Connection Refused, bad user name or password";
static CRC_5_MESSAGE: &'static str = "0x05 Connection Refused, not authorized";

bitflags! {
    pub struct PacketFlags: u8 {
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

impl From<QualityOfService> for PacketFlags {
    fn from(value: QualityOfService) -> PacketFlags {
        match value {
            QualityOfService::QoS0 => PacketFlags::empty(),
            QualityOfService::QoS1 => PacketFlags::QOS1,
            QualityOfService::QoS2 => PacketFlags::QOS2
        }
    }
}

bitflags! {
    pub struct ConnFlags: u8 {
        const USERNAME    = 0b10000000;
        const PASSWORD    = 0b01000000;
        const WILL_RETAIN = 0b00100000;
        const WILL_QOS2   = 0b00010000;
        const WILL_QOS1   = 0b00001000;
        const WILL_FLAG   = 0b00000100;
        const CLEAN_SESS  = 0b00000010;
    }
}

impl From<QualityOfService> for ConnFlags {
    fn from(value: QualityOfService) -> ConnFlags {
        match value {
            QualityOfService::QoS0 => ConnFlags::empty(),
            QualityOfService::QoS1 => ConnFlags::WILL_QOS1,
            QualityOfService::QoS2 => ConnFlags::WILL_QOS2
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

enum_from_primitive! {
    #[derive(Clone, Copy, Debug)]
    pub enum PacketType {
        Connect     = 1,
        ConnAck  = 2,
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

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum ConnRetCode {
        Accepted          = 0,
        BadProtoVersion   = 1,
        ClientIdRejected  = 2,
        ServerUnavailable = 3,
        BadCredentials    = 4,
        Unauthorized      = 5
    }
}

impl ConnRetCode {
    pub fn is_ok(&self) -> bool {
        match self {
            &ConnRetCode::Accepted => true,
            _ => false
        }
    }

    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }
}

impl From<ConnRetCode> for u8 {
    fn from(data: ConnRetCode) -> u8 {
        match data {
            ConnRetCode::Accepted          => 0,
            ConnRetCode::BadProtoVersion   => 1,
            ConnRetCode::ClientIdRejected  => 2,
            ConnRetCode::ServerUnavailable => 3,
            ConnRetCode::BadCredentials    => 4,
            ConnRetCode::Unauthorized      => 5
        }
    }
}

impl fmt::Display for ConnRetCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &ConnRetCode::Accepted => write!(f, "{}", CRC_0_MESSAGE),
            &ConnRetCode::BadProtoVersion => write!(f, "{}", CRC_1_MESSAGE),
            &ConnRetCode::ClientIdRejected => write!(f, "{}", CRC_2_MESSAGE),
            &ConnRetCode::ServerUnavailable => write!(f, "{}", CRC_3_MESSAGE),
            &ConnRetCode::BadCredentials => write!(f, "{}", CRC_4_MESSAGE),
            &ConnRetCode::Unauthorized => write!(f, "{}", CRC_5_MESSAGE)
        }
    }
}

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[allow(non_camel_case_types)]
    pub enum ProtoLvl {
        V3_1_1 = 4
    }
}

impl ProtoLvl {
    pub fn as_u8(&self) -> u8 {
        match *self {
            ProtoLvl::V3_1_1 => 4
        }
    }
}

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum QualityOfService {
        QoS0 = 0,
        QoS1 = 1,
        QoS2 = 2
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
            &QualityOfService::QoS2 => write!(f, "QOS2")
        }
    }
}

impl Encodable for QualityOfService {
    fn encode<B: BufMut>(&self, out: &mut B) {
            out.put_u8(*self as u8)
    }

    fn encoded_length(&self) -> usize { 1 }
}

enum_from_primitive!{
    #[derive(Clone, Debug, Copy)]
    pub enum SubAckReturnCode {
        SuccessQoS0 = 0,
        SuccessQoS1 = 1,
        SuccessQoS2 = 2,
        Failure     = 128,
    }
}

impl SubAckReturnCode {
    pub fn qos(&self) -> Option<QualityOfService> {
        use self::SubAckReturnCode::*;
        match self {
            &SuccessQoS0 => Some(QualityOfService::QoS0),
            &SuccessQoS1 => Some(QualityOfService::QoS1),
            &SuccessQoS2 => Some(QualityOfService::QoS2),
            &Failure => None,
        }
    }
}

impl Encodable for SubAckReturnCode {
    fn encode<B: BufMut>(&self, out: &mut B) {
            out.put_u8(*self as u8)
    }

    fn encoded_length(&self) -> usize { 1 }
}

pub trait Encodable {
    /// Encodes the packet section into a buffer.
    fn encode<B: BufMut>(&self, out: &mut B);
    /// Returns the size of the encoded section.
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

impl Encodable for &str {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u16_be(self.len() as u16);
        out.put_slice(self.as_bytes());
    }

    fn encoded_length(&self) -> usize {
        2 + self.len()
    }
}

pub struct SubscriptionTuple<'a>(pub &'a str, pub QualityOfService);

impl<'a> Encodable for SubscriptionTuple<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.0.encode(out);
        self.1.encode(out);
    }
    
    fn encoded_length(&self) -> usize {
        self.0.encoded_length() + self.1.encoded_length()
    }
} 

#[derive(Builder, Clone)]
pub struct LWTMessage<T: AsRef<str>, P: AsRef<[u8]>> {
    pub topic: T,
    #[builder(default = QualityOfService::QoS0)]
    pub qos: QualityOfService,
    #[builder(default = "false")]
    pub retain: bool,
    #[builder(default = [])]
    pub message: P
}

impl<T: AsRef<str>, P: AsRef<[u8]>> LWTMessage<T, P> {
    pub fn from_flags(flags: ConnFlags, t: T, m: P) -> LWTMessage<T, P> {
        LWTMessage {
            topic: t,
            qos: flags.into(),
            retain: flags.intersects(ConnFlags::WILL_RETAIN),
            message: m
        }
    }

    pub fn as_ref(&self) -> LWTMessage<&str, &[u8]> {
        LWTMessage {
            topic: self.topic.as_ref(),
            qos: self.qos,
            retain: self.retain,
            message: self.message.as_ref()
        }
    }

    pub(crate) fn connect_flags(&self) -> ConnFlags {
        let mut flags: ConnFlags = self.qos.into();
        flags.set(ConnFlags::WILL_RETAIN, self.retain);
        flags
    }
}

impl Encodable for LWTMessage<&str, &[u8]> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.topic.encode(out);
        self.message.encode(out);
    }
    
    fn encoded_length(&self) -> usize {
        self.topic.encoded_length() + self.message.encoded_length()
    }
}

/// Container for MQTT credentials, with a username and optional password.
pub struct Credentials<U: AsRef<str>, P: AsRef<[u8]>> {
    pub username: U,
    pub password: Option<P>
}

impl<U: AsRef<str>, P: AsRef<[u8]>> Credentials<U, P> {
    /// Converts Credentials<U, P> to Credentials<&str, &[u8]>
    pub fn as_ref(&self) -> Credentials<&str, &[u8]> {
        Credentials {
            username: self.username.as_ref(),
            password: self.password.as_ref().map(|p| p.as_ref())
        }
    }
}

impl Encodable for Credentials<&str, &[u8]> {
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
