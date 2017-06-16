mod types;
mod parsers;

pub use self::types::*;

use ::errors::{ErrorKind as MqttErrorKind, Result as MqttResult};
use ::errors::proto;
use ::nom::IResult;
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

#[derive(Clone, Serialize, Deserialize)]
pub struct MqttPacket {
    pub ty: PacketType,
    pub flags: PacketFlags,
    pub headers: HeaderMap,
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
                ty: ty,
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

    pub fn connect_packet(version: ProtocolLevel, lwt: Option<LWTMessage>,
        creds: Credentials<MqttString>, clean: bool, keep_alive: u16, id: Option<MqttString>) -> MqttPacket {

        let mut flags = ConnectFlags::empty();
        let mut headers = HeaderMap::new();

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

        headers.insert("protocol_name".into(), Headers::ProtoName);
        headers.insert("protocol_level".into(), Headers::ProtoLevel(version));
        headers.insert("connect_flags".into(), Headers::ConnFlags(flags));
        headers.insert("keep_alive".into(), Headers::KeepAlive(keep_alive));

        MqttPacket {
            ty: PacketType::Connect,
            flags: PacketFlags::empty(),
            headers: headers,
            payload: payload
        }
    }

    pub fn publish_packet(flags: PacketFlags, topic: MqttString, id: u16,
        msg: Bytes) -> MqttPacket {

        let mut headers = HeaderMap::new();
        headers.insert("topic_name".into(), Headers::TopicName(topic));

        if flags.contains(QOS1 | QOS2) {
            headers.insert("packet_id".into(), Headers::PacketId(id));
        }

        MqttPacket {
            ty: PacketType::Publish,
            flags: flags,
            headers: headers,
            payload: Payload::Application(msg.to_vec())
        }
    }

    pub fn pub_ack_packet(id: u16) -> MqttPacket {
        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));
        MqttPacket {
            ty: PacketType::PubAck,
            flags: PacketFlags::empty(),
            headers: headers,
            payload: Payload::None
        }
    }

    pub fn pub_rec_packet(id: u16) -> MqttPacket {
        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));
        MqttPacket {
            ty: PacketType::PubRec,
            flags: PacketFlags::empty(),
            headers: headers,
            payload: Payload::None
        }
    }

    pub fn pub_rel_packet(id: u16) -> MqttPacket {
        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));
        MqttPacket {
            ty: PacketType::PubRel,
            flags: PUBREL,
            headers: headers,
            payload: Payload::None
        }
    }

    pub fn pub_comp_packet(id: u16) -> MqttPacket {
        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));
        MqttPacket {
            ty: PacketType::PubComp,
            flags: PacketFlags::empty(),
            headers: headers,
            payload: Payload::None
        }
    }

    pub fn subscribe_packet(id: u16, subscriptions: Vec<Subscription>) -> MqttPacket {
        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));

        MqttPacket {
            ty: PacketType::Subscribe,
            flags: SUB,
            headers: headers,
            payload: Payload::Subscribe(subscriptions)
        }
    }

    pub fn unsubscribe_packet(id: u16, subscriptions: Vec<Subscription>) -> MqttPacket {
        let mut subs = Vec::new();
        for sub in subscriptions {
            subs.push(sub.topic);
        }

        let mut headers = HeaderMap::new();
        headers.insert("packet_id".into(), Headers::PacketId(id));

        MqttPacket {
            ty: PacketType::Unsubscribe,
            flags: UNSUB,
            headers: headers,
            payload: Payload::Unsubscribe(subs)
        }
    }

    pub fn ping_req_packet() -> MqttPacket {
        MqttPacket {
            ty: PacketType::PingReq,
            flags: PacketFlags::empty(),
            headers: HeaderMap::new(),
            payload: Payload::None
        }
    }

    pub fn disconnect_packet() -> MqttPacket {
        MqttPacket {
            ty: PacketType::Disconnect,
            flags: PacketFlags::empty(),
            headers: HeaderMap::new(),
            payload: Payload::None
        }
    }

    pub fn encode(&self) -> Option<Bytes> {
        let mut buf = BytesMut::with_capacity(4096);
        let ty_flags: u8 = (((self.ty as u32) as u8) << 4) + self.flags.bits();
        buf.put_u8(ty_flags);
        let headers_bytes = self.encode_headers();
        let payload_bytes = self.payload.encode();
        if let Some(vle_bytes) = encode_vle(headers_bytes.len() + payload_bytes.len()) {
            buf.put(vle_bytes);
            buf.put(headers_bytes);
            buf.put(payload_bytes);
            Some(buf.freeze())
        } else {
            None
        }
    }

    fn encode_headers(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(64);
        for header in self.headers.values() {
            buf.put(header.encode())
        }
        buf.freeze()
    }
}
