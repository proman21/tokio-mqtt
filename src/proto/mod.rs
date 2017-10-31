mod types;
mod parsers;
mod headers;

pub use self::types::*;
pub use self::headers::*;

use ::errors::{ErrorKind as MqttErrorKind, Result as MqttResult};
use ::errors::proto;
use ::nom::IResult;
use ::linked_hash_map::LinkedHashMap;
use ::bytes::{Bytes, BytesMut, BufMut};
use self::parsers::packet;

fn encode_vle(num: usize) -> Option<Bytes> {
    let mut collect: BytesMut = BytesMut::with_capacity(4);
    let mut val: usize = num;

    if num > 268_435_455 {
        return None;
    }

    loop {
        let mut enc_byte: u8 = (val % 128) as u8;
        val /= 128;
        if val > 0 {
            enc_byte |= 128;
        }
        collect.put_u8(enc_byte);
        if val <= 0 {
            break;
        }
    }
    return Some(collect.freeze());
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Headers {
    data: LinkedHashMap<String, Vec<u8>>
}

impl Headers {
    pub fn new() -> Headers {
        Headers {
            data: LinkedHashMap::new()
        }
    }

    pub fn set<H: Header>(&mut self, value: H) {
        unimplemented!()
    }

    pub fn set_raw(&mut self, name: &'static str, value: &[u8]) {
        self.data.insert(name.into(), Vec::from(value));
    }

    pub fn get<H: Header>(&self) -> Option<H>{
        unimplemented!()
    }

    pub fn encode(&self, out: &mut BytesMut) {
        for header in self.data.values() {
            out.put_slice(header);
        }
    }
}

pub trait Header: Sized {
    fn header_name() -> &'static str;
    fn parse_header(raw: &Bytes) -> MqttResult<Self>;
    fn fmt_header(&self, out: &mut BytesMut) -> MqttResult<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MqttPacket {
    pub ty: PacketType,
    pub flags: PacketFlags,
    pub headers: Headers,
    pub payload: Payload
}

impl MqttPacket {
    /// Attempts to decode a MQTT Control Packet from the provided slice of bytes.
    ///
    /// If there is a fully formed packet in the slice, `Ok(Some((MqttPacket, Vec<u8>)))` will be
    /// returned, containing the formed packet and the rest of the slice.
    ///
    /// If there is not enough bytes in the slice to create a fully formed packet, `Ok(None)` will
    /// be returned.
    ///
    /// If an error occurs decoding the bytes, an error will be returned.
    pub fn from_slice<B: AsRef<[u8]>>(data: B) -> MqttResult<Option<(MqttPacket, Vec<u8>)>> {
        let slice = data.as_ref();
        match packet(slice) {
            IResult::Done(rest, (ty, fl, hd, pl)) => Ok(Some((MqttPacket {
                ty,
                flags: fl,
                headers: hd,
                payload: pl
            }, rest.into()))),
            IResult::Incomplete(_) => return Ok(None),
            IResult::Error(e) => bail!(MqttErrorKind::PacketDecodingError)
        }
    }

    pub fn validate(&self) -> MqttResult<()> {
        use self::PacketType::*;
        match self.ty {
            ConnAck | PubAck | PubRec | PubComp | SubAck | UnsubAck | PingResp => {
                if self.flags.bits() == 0 {
                    Ok(())
                } else {
                    bail!(proto::ErrorKind::InvalidPacket("Invalid packet flags".into()))
                }
            },
            PubRel => {
                if self.flags.bits() == 0b0010 {
                    Ok(())
                } else {
                    bail!(proto::ErrorKind::InvalidPacket("Invalid packet flags".into()))
                }
            },
            _ => Ok(())
        }
    }

    pub fn connect_packet(version: ProtoLvl, lwt: Option<LWTMessage>,
        creds: Credentials<MqttString>, clean: bool, keep_alive: u16, id: Option<MqttString>) -> MqttPacket {

        let mut flags = ConnFlags::empty();
        let mut headers = Headers::new();

        let last_will = if let Some(will) = lwt {
            flags.insert(will.encode_flags());
            Some((will.topic, will.message.to_vec()))
        } else {
            None
        };

        if let Some((_, ref pass)) = creds {
            flags.insert(USERNAME);

            if let &Some(_) = pass {
                flags.insert(PASSWORD);
            }
        }

        if clean {
            flags.insert(CLEAN_SESS);
        }

        let payload = Payload::Connect(
            id.unwrap_or(MqttString::new()),
            last_will,
            creds
        );

        headers.set(ProtocolName::new(MqttString::from_str("MQTT").expect("Valid MQTT string is invalid")));
        headers.set(ProtocolLevel::new(version));
        headers.set(ConnectFlags::new(flags));
        headers.set(KeepAlive::new(keep_alive));

        MqttPacket {
            ty: PacketType::Connect,
            flags: PacketFlags::empty(),
            headers,
            payload
        }
    }

    pub fn publish_packet(flags: PacketFlags, topic: MqttString, id: u16,
        msg: Bytes) -> MqttPacket {

        let mut headers = Headers::new();
        headers.set(TopicName::new(topic));

        if flags.contains(QOS1 | QOS2) {
            headers.set(PacketId::new(id));
        }

        MqttPacket {
            ty: PacketType::Publish,
            flags,
            headers,
            payload: Payload::Application(msg.to_vec())
        }
    }

    pub fn pub_ack_packet(id: u16) -> MqttPacket {
        let mut headers = Headers::new();
        headers.set(PacketId::new(id));
        MqttPacket {
            ty: PacketType::PubAck,
            flags: PacketFlags::empty(),
            headers,
            payload: Payload::None
        }
    }

    pub fn pub_rec_packet(id: u16) -> MqttPacket {
        let mut headers = Headers::new();
        headers.set(PacketId::new(id));
        MqttPacket {
            ty: PacketType::PubRec,
            flags: PacketFlags::empty(),
            headers,
            payload: Payload::None
        }
    }

    pub fn pub_rel_packet(id: u16) -> MqttPacket {
        let mut headers = Headers::new();
        headers.set(PacketId::new(id));
        MqttPacket {
            ty: PacketType::PubRel,
            flags: PUBREL,
            headers,
            payload: Payload::None
        }
    }

    pub fn pub_comp_packet(id: u16) -> MqttPacket {
        let mut headers = Headers::new();
        headers.set(PacketId::new(id));
        MqttPacket {
            ty: PacketType::PubComp,
            flags: PacketFlags::empty(),
            headers,
            payload: Payload::None
        }
    }

    pub fn subscribe_packet(id: u16, subscriptions: Vec<Subscription>) -> MqttPacket {
        let mut headers = Headers::new();
        headers.set(PacketId::new(id));

        MqttPacket {
            ty: PacketType::Subscribe,
            flags: SUB,
            headers,
            payload: Payload::Subscribe(subscriptions)
        }
    }

    pub fn unsubscribe_packet(id: u16, subscriptions: Vec<Subscription>) -> MqttPacket {
        let mut subs = Vec::new();
        for sub in subscriptions {
            subs.push(sub.topic);
        }

        let mut headers = Headers::new();
        headers.set(PacketId::new(id));

        MqttPacket {
            ty: PacketType::Unsubscribe,
            flags: UNSUB,
            headers,
            payload: Payload::Unsubscribe(subs)
        }
    }

    pub fn ping_req_packet() -> MqttPacket {
        MqttPacket {
            ty: PacketType::PingReq,
            flags: PacketFlags::empty(),
            headers: Headers::new(),
            payload: Payload::None
        }
    }

    pub fn disconnect_packet() -> MqttPacket {
        MqttPacket {
            ty: PacketType::Disconnect,
            flags: PacketFlags::empty(),
            headers: Headers::new(),
            payload: Payload::None
        }
    }

    pub fn encode(&self) -> Option<Bytes> {
        let mut buf = BytesMut::with_capacity(4096);
        let ty_flags: u8 = (((self.ty as u32) as u8) << 4) + self.flags.bits();
        buf.put_u8(ty_flags);
        let mut payload_buf = BytesMut::with_capacity(2048);
        self.encode_headers(&mut payload_buf);
        self.payload.encode(&mut payload_buf);
        if let Some(vle_bytes) = encode_vle(payload_buf.len()) {
            buf.put(vle_bytes);
            buf.put(payload_buf);
            Some(buf.freeze())
        } else {
            None
        }
    }

    fn encode_headers(&self, out: &mut BytesMut) {
        self.headers.encode(out);
    }
}
