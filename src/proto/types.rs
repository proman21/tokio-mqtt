use std::ops::Deref;
use std::fmt;
use ::bytes::{Bytes, BytesMut, BigEndian, BufMut};
use ::errors::{Error, ErrorKind};
use ::linked_hash_map::LinkedHashMap;

static CRC_0_MESSAGE: &'static str = "0x00 Connection Accepted";
static CRC_1_MESSAGE: &'static str = "0x01 Connection Refused, unacceptable protocol version";
static CRC_2_MESSAGE: &'static str = "0x02 Connection Refused, identifier rejected";
static CRC_3_MESSAGE: &'static str = "0x03 Connection Refused, Server unavailable";
static CRC_4_MESSAGE: &'static str = "0x04 Connection Refused, bad user name or password";
static CRC_5_MESSAGE: &'static str = "0x05 Connection Refused, not authorized";

pub type HeaderMap = LinkedHashMap<String, Headers>;

bitflags! {
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
}

bitflags! {
    pub flags ConnectFlags: u8 {
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

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum ConnectReturnCode {
        Accepted          = 0,
        BadProtoVersion   = 1,
        ClientIdRejected  = 2,
        ServerUnavailable = 3,
        BadCredentials    = 4,
        Unauthorized      = 5
    }
}

impl ConnectReturnCode {
    pub fn is_ok(&self) -> bool {
        match self {
            &ConnectReturnCode::Accepted => true,
            _ => false
        }
    }

    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }
}

impl fmt::Display for ConnectReturnCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &ConnectReturnCode::Accepted => write!(f, "{}", CRC_0_MESSAGE),
            &ConnectReturnCode::BadProtoVersion => write!(f, "{}", CRC_1_MESSAGE),
            &ConnectReturnCode::ClientIdRejected => write!(f, "{}", CRC_2_MESSAGE),
            &ConnectReturnCode::ServerUnavailable => write!(f, "{}", CRC_3_MESSAGE),
            &ConnectReturnCode::BadCredentials => write!(f, "{}", CRC_4_MESSAGE),
            &ConnectReturnCode::Unauthorized => write!(f, "{}", CRC_5_MESSAGE)
        }
    }
}

enum_from_primitive! {
    #[derive(Clone, Copy, Debug, PartialEq)]
    #[allow(non_camel_case_types)]
    pub enum ProtocolLevel {
        V3_1_1 = 4
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
    #[derive(Clone, Copy)]
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

    pub fn encode_flags(&self) -> ConnectFlags {
        let mut flags = ConnectFlags::from_bits_truncate(self.qos.into());
        if self.retain {
            flags.insert(WILL_RETAIN)
        }
        flags
    }

    pub fn encode_topic(&self) -> Bytes {
        self.topic.encode()
    }

    pub fn encode_message(&self) -> Bytes {
        self.message.clone()
    }
}

#[derive(Clone)]
pub enum Headers {
    ConnFlags(ConnectFlags),
    ConnAckFlags(ConnAckFlags),
    ConnRetCode(ConnectReturnCode),
    PacketId(u16),
    ProtoName,
    ProtoLevel(ProtocolLevel),
    KeepAlive(u16),
    TopicName(MqttString)
}

impl Headers {
    pub fn encode(&self) -> Bytes {
        let mut b = BytesMut::with_capacity(32);
        match *self {
            Headers::ConnFlags(ref fl) => b.put_u8(fl.bits()),
            Headers::ConnAckFlags(ref fl) => b.put_u8(fl.bits()),
            Headers::ConnRetCode(ref c) => b.put_u8((*c as u32) as u8),
            Headers::PacketId(ref id) => b.put_u16::<BigEndian>(*id),
            Headers::ProtoName => b.put(MqttString::from_str("MQTT").unwrap().encode()),
            Headers::ProtoLevel(ref lvl) => b.put_u8((*lvl as u32) as u8),
            Headers::KeepAlive(ref ka) => b.put_u16::<BigEndian>(*ka),
            Headers::TopicName(ref s) => b.put(s.encode())
        }
        b.freeze()
    }
}

#[derive(Clone)]
pub struct MqttString(String);

impl MqttString {
    pub fn new() -> MqttString {
        MqttString(String::new())
    }

    pub fn from_str(s: &str) -> Result<MqttString, Error> {
        if s.len() > 0xFFFF {
            bail!(ErrorKind::StringConversionError)
        } else {
            Ok(MqttString(String::from(s)))
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut b = BytesMut::with_capacity(self.len() + 2);
        b.put_u16::<BigEndian>(self.len() as u16);
        b.put(&self.0);
        b.freeze()
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for MqttString {
    type Target = String;

    fn deref(&self) -> &String {
        &self.0
    }
}


pub type Credentials<T> = Option<(T, Option<T>)>;

pub struct Subscription {
    pub topic: MqttString,
    pub qos: QualityOfService
}

impl Subscription {
    pub fn encode_all(&self) -> Bytes {
        let mut bytes = BytesMut::from(self.topic.encode());
        bytes.put_u8(self.qos.into());
        bytes.freeze()
    }

    pub fn encode_topic(&self) -> Bytes {
        BytesMut::from(self.topic.encode()).freeze()
    }
}

pub enum Payload {
    Connect(MqttString, Option<(MqttString, Bytes)>, Credentials<MqttString>),
    Subscribe(Vec<Subscription>),
    SubAck(Vec<SubAckReturnCode>),
    Unsubscribe(Vec<MqttString>),
    Application(Bytes),
    None
}

impl Payload {
    pub fn encode(&self) -> Bytes {
        let mut collect = BytesMut::with_capacity(4096);
        match self {
            &Payload::Connect(ref cid, ref will, ref creds) => {
                collect.extend(cid.encode());
                if let &Some((ref topic, ref msg)) = will {
                    collect.extend(topic.encode());
                    collect.extend(msg);
                }
                if let &Some((ref user, ref p)) = creds {
                    collect.extend(user.encode());

                    if let &Some(ref pass) = p {
                        collect.extend(pass.encode());
                    }
                }
            },
            &Payload::Subscribe(ref subs) => {
                for sub in subs {
                    collect.extend(sub.topic.encode());
                    collect.extend(vec![u8::from(sub.qos)]);
                }
            },
            &Payload::Unsubscribe(ref unsubs) => {
                for unsub in unsubs {
                    collect.extend(unsub.encode());
                }
            }
            _ => {}
        };
        collect.freeze()
    }
}

impl<'a> From<&'a [u8]> for Payload {
    fn from(b: &'a [u8]) -> Payload {
        Payload::Application(Bytes::from(b))
    }
}
