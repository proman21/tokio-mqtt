use std::ops::Deref;
use std::fmt;
use ::bytes::{Bytes, BytesMut, BigEndian, BufMut};
use ::errors::{Error, ErrorKind};
use ::linked_hash_map::LinkedHashMap;
use ::errors::Result;

static CRC_0_MESSAGE: &'static str = "0x00 Connection Accepted";
static CRC_1_MESSAGE: &'static str = "0x01 Connection Refused, unacceptable protocol version";
static CRC_2_MESSAGE: &'static str = "0x02 Connection Refused, identifier rejected";
static CRC_3_MESSAGE: &'static str = "0x03 Connection Refused, Server unavailable";
static CRC_4_MESSAGE: &'static str = "0x04 Connection Refused, bad user name or password";
static CRC_5_MESSAGE: &'static str = "0x05 Connection Refused, not authorized";

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub flags PacketFlags: u8 {
        const DUP  = 0b1000,
        const QOS2 = 0b0100,
        const QOS1 = 0b0010,
        const RET  = 0b0001,

        const CONNACK = QOS1.bits,
        const PUBREL  = QOS1.bits,
        const SUB     = QOS1.bits,
        const UNSUB   = QOS1.bits,
    }
}

impl PacketFlags {
    pub fn qos(&self) -> QualityOfService {
        if self.intersects(QOS2 | QOS1) {
            if self.contains(QOS2) {
                QualityOfService::QoS2
            } else {
                QualityOfService::QoS1
            }
        } else {
            QualityOfService::QoS0
        }
    }

    pub fn is_retain(&self) -> bool {
        self.intersects(RET)
    }

    pub fn is_duplicate(&self) -> bool {
        self.intersects(DUP)
    }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub flags ConnFlags: u8 {
        const USERNAME    = 0b10000000,
        const PASSWORD    = 0b01000000,
        const WILL_RETAIN = 0b00100000,
        const WILL_QOS2   = 0b00010000,
        const WILL_QOS1   = 0b00001000,
        const WILL_FLAG   = 0b00000100,
        const CLEAN_SESS  = 0b00000010,
    }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub flags ConnAckFlags: u8 {
        const SP = 0b0001,
    }
}

impl ConnAckFlags {
    pub fn is_clean(&self) -> bool {
        self.contains(SP)
    }
}

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

    pub fn as_u8(&self) -> u8 {
        match *self {
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
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    pub enum QualityOfService {
        QoS0 = 0,
        QoS1 = 1,
        QoS2 = 2
    }
}

impl From<QualityOfService> for u8 {
    fn from(data: QualityOfService) -> u8 {
        (data as u32) as u8
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

enum_from_primitive!{
    #[derive(Clone, Copy, Serialize, Deserialize)]
    pub enum SubAckReturnCode {
        SuccessQoS0 = 0,
        SuccessQoS1 = 1,
        SuccessQoS2 = 2,
        Failure     = 128,
    }
}

impl SubAckReturnCode {
    pub fn is_ok(&self) -> bool {
        match self {
            &SubAckReturnCode::Failure => false,
            _ => true
        }
    }
}

pub struct LWTMessage {
    pub topic: MqttString,
    pub qos: QualityOfService,
    pub retain: bool,
    pub message: Bytes
}

impl LWTMessage {
    pub fn new(t: MqttString, q: QualityOfService, r: bool, m: Bytes) -> LWTMessage {
        LWTMessage {
            topic: t,
            qos: q,
            retain: r,
            message: m
        }
    }

    pub fn encode_flags(&self) -> ConnFlags {
        let mut flags = ConnFlags::from_bits_truncate(self.qos.into());
        if self.retain {
            flags.insert(WILL_RETAIN);
        }
        flags
    }

    pub fn encode_topic(&self, out: &mut BytesMut) {
        self.topic.encode(out);
    }

    pub fn encode_message(&self, out: &mut BytesMut) {
        out.reserve(self.message.len());
        out.put(self.message.slice_from(0));
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MqttString(String);

impl MqttString {
    pub fn new() -> MqttString {
        MqttString(String::new())
    }

    pub fn from_str(s: &str) -> Result<MqttString> {
        if s.len() > 0xFFFF {
            bail!(ErrorKind::StringConversionError)
        } else {
            Ok(MqttString(String::from(s)))
        }
    }

    pub fn from_str_lossy(s: &str) -> MqttString {
        let mut string = String::from(s);
        string.truncate(0xFFFF);
        MqttString(string)
    }

    pub fn encode(&self, out: &mut BytesMut) {
        out.reserve(self.len() + 2);
        out.put_u16::<BigEndian>(self.len() as u16);
        out.put_slice(self.as_bytes());
    }
}

impl Deref for MqttString {
    type Target = String;

    fn deref(&self) -> &String {
        &self.0
    }
}

impl Into<String> for MqttString {
    fn into(self) -> String {
        self.0
    }
}

pub type Credentials<T> = Option<(T, Option<T>)>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub topic: MqttString,
    pub qos: QualityOfService
}

impl Subscription {
    pub fn encode_all(&self, out: &mut BytesMut) {
        out.reserve(self.topic.len() + 1);
        self.topic.encode(out);
        out.put_u8(self.qos.into());
    }

    pub fn encode_topic(&self, out: &mut BytesMut) {
        self.topic.encode(out);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Payload {
    Connect(MqttString, Option<(MqttString, Vec<u8>)>, Credentials<MqttString>),
    Subscribe(Vec<Subscription>),
    SubAck(Vec<SubAckReturnCode>),
    Unsubscribe(Vec<MqttString>),
    Application(Vec<u8>),
    None
}

impl Payload {
    pub fn encode(&self, out: &mut BytesMut) {
        match self {
            &Payload::Connect(ref cid, ref will, ref creds) => {
                cid.encode(out);
                if let &Some((ref topic, ref msg)) = will {
                    topic.encode(out);
                    out.reserve(msg.len());
                    out.put(msg);
                }
                if let &Some((ref user, ref p)) = creds {
                    user.encode(out);

                    if let &Some(ref pass) = p {
                        pass.encode(out);
                    }
                }
            },
            &Payload::Subscribe(ref subs) => {
                for sub in subs {
                    sub.encode_all(out);
                }
            },
            &Payload::Unsubscribe(ref unsubs) => {
                for unsub in unsubs {
                    unsub.encode(out);
                }
            }
            _ => {}
        }
    }
}

impl<'a> From<&'a [u8]> for Payload {
    fn from(b: &'a [u8]) -> Payload {
        Payload::Application(Vec::from(b))
    }
}
