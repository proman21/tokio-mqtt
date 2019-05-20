use bytes::BufMut;
use parsers::*;
use types::*;
use errors::*;
use super::MqttPacket;

fn encode_vle<B: BufMut>(num: usize, out: &mut B) -> Result<()> {
    let mut val: usize = num;

    if num > 268_435_455 {
        return Err(Error::PacketTooBig(num));
    }

    loop {
        let mut enc_byte: u8 = (val % 128) as u8;
        val /= 128;
        if val > 0 {
            enc_byte |= 128;
        }
        out.put_u8(enc_byte);
        if val <= 0 {
            break;
        }
    }
    Ok(())
}

impl<'a> MqttPacket<'a> {
    pub fn packet_type(&self) -> PacketType {
        use self::MqttPacket::*;

        match self {
            Connect {.. } => PacketType::Connect,
            ConnectAck { .. } => PacketType::ConnectAck,
            Publish { .. } => PacketType::Publish,
            PubAck{ .. } => PacketType::PubAck,
            PubRec{ .. } => PacketType::PubRec,
            PubRel{ .. } => PacketType::PubRel,
            PubComp{ .. } => PacketType::PubComp,
            Subscribe{ .. } => PacketType::Subscribe,
            SubAck { .. } => PacketType::SubAck,
            Unsubscribe { .. } => PacketType::Unsubscribe,
            UnsubAck { .. } => PacketType::UnsubAck,
            PingReq => PacketType::PingReq,
            PingResp => PacketType::PingResp,
            Disconnect => PacketType::Disconnect
        }
    }
    
    pub fn packet_flags(&self) -> PacketFlags {
        use ::MqttPacket::*;

        match self {
            Connect { .. } | ConnectAck { .. } | PubAck{ .. } | PubRec{ .. } | PubRel{ .. } | PubComp{ .. } | 
                SubAck { .. } | UnsubAck { .. } | PingReq | PingResp | Disconnect => PacketFlags::empty(),
            Publish { dup, qos, retain, ..} => {
                let mut flags = PacketFlags::from(*qos);
                flags.set(PacketFlags::DUP, *dup);
                flags.set(PacketFlags::RET, *retain);
                flags
            },
            Subscribe{ .. } | Unsubscribe {.. } => PacketFlags::QOS1
        }
    }

    /// Attempts to decode a MQTT Control Packet from the provided slice of bytes.
    ///
    /// If there is a fully formed packet in the slice, a tuple will be returned that contains the
    /// packet and the rest of the slice.
    ///
    /// If there is not enough bytes in the slice to create a fully formed packet, `Ok(None)` will
    /// be returned.
    ///
    /// If an error occurs decoding the bytes, an error will be returned.
    pub fn from_buf<B: AsRef<[u8]>>(buf: &'a B) -> Result<Option<(&'a [u8], MqttPacket<'a>)>> {
        MqttPacket::from_slice(buf.as_ref())
    }
    
    // Non monomorphized code path for parsing to reduce library size.
    fn from_slice(buf: &'a [u8]) -> Result<Option<(&'a [u8], MqttPacket<'a>)>> {
        use self::PacketType::*;
        let (rest, (ty, fl, re)) = fixed_header(buf.as_ref()).map_err(|e| Error::ParseFailure(e.into_error_kind()))?;

        if rest.len() < re {
            return Ok(None);
        }
        let (remain, after) = rest.split_at(re);
        let (_, packet) = match ty {
            Connect => connect_packet(remain),
            ConnectAck => connect_ack_packet(remain),
            Publish => publish_packet(remain, fl),
            PubAck => packet_id_header(remain, |id| MqttPacket::PubAck {packet_id: id}),
            PubRec => packet_id_header(remain, |id| MqttPacket::PubRec {packet_id: id}),
            PubRel => packet_id_header(remain, |id| MqttPacket::PubRel {packet_id: id}),
            PubComp => packet_id_header(remain, |id| MqttPacket::PubComp {packet_id: id}),
            Subscribe => subscribe_packet(remain),
            SubAck => sub_ack_packet(remain),
            Unsubscribe => unsubscribe_packet(remain),
            UnsubAck => packet_id_header(remain, |id| MqttPacket::UnsubAck {packet_id: id}),
            PingReq => complete!(remain, value!(MqttPacket::PingReq)),
            PingResp => complete!(remain, value!(MqttPacket::PingResp)),
            Disconnect => complete!(remain, value!(MqttPacket::Disconnect))
        }.map_err(|e| Error::ParseFailure(e.into_error_kind()))?;

        Ok(Some((after, packet)))
    }
    
    /// Attempt to encode the packet into a buffer.
    /// If the packet size is too big to be encoded, an error will be raised.
    ///
    /// # Panics
    /// Panics if `out` does not have enough capacity to contain the packet. Use `len()` to determine the size of the
    /// encoded packet.
    pub fn encode<B: BufMut>(&self, out: &mut B) -> Result<()> {
        use self::MqttPacket::*;
        
        out.put_u8(((self.packet_type() as u8) << 4) + self.packet_flags().bits());

        encode_vle(self.encoded_length(), out)?;
        match self {
            Connect{ protocol_level, clean_session, keep_alive, client_id, lwt, credentials } => {
                out.put_u16_be(4);
                out.put_slice(b"MQTT");
        
                out.put_u8(*protocol_level as u8);
                
                let mut flags = lwt.as_ref().map_or(ConnFlags::empty(), |f| f.connect_flags());
                flags.set(ConnFlags::CLEAN_SESS, *clean_session);
                if let Some(c) = credentials {
                    flags.insert(ConnFlags::USERNAME);
                    flags.set(ConnFlags::PASSWORD, c.password.is_some());
                }
                out.put_u8(flags.bits());
        
                out.put_u16_be(*keep_alive);
                
                client_id.encode(out);
                if let Some(lwt) = lwt {
                    lwt.encode(out)
                }
                
                if let Some(c) = credentials {
                    c.encode(out);
                }
            },
            ConnectAck{ session_present, connect_return_code } => {
                let mut flags = ConnAckFlags::empty();
                flags.set(ConnAckFlags::SP, *session_present);
                out.put_u8(flags.bits());
                out.put_u8(*connect_return_code as u8);
            },
            Publish {topic_name, packet_id, message, ..} => {
                 topic_name.encode(out);
                 if let Some(id) = packet_id {
                     out.put_u16_be(*id);
                 }
                 out.put_slice(message);
            },
            PubAck{ packet_id } | PubRec{ packet_id } | PubRel{ packet_id } | PubComp{ packet_id } |
                UnsubAck { packet_id} => {
                out.put_u16_be(*packet_id);
            },
            Subscribe{ packet_id, subscriptions } => {
                out.put_u16_be(*packet_id);
                subscriptions.encode(out);
            },
            SubAck{ packet_id, return_codes } => {
                out.put_u16_be(*packet_id);
                return_codes.encode(out);
            },
            Unsubscribe{ packet_id, topics } => {
                out.put_u16_be(*packet_id);
                topics.encode(out);
            },
            PingReq | PingResp | Disconnect => {}
            _ => unimplemented!()
        }

        Ok(())
    }
    
    fn encoded_length(&self) -> usize {
        use self::MqttPacket::*;
        
        match self {
            Connect{ client_id, lwt, credentials, .. } => {
                10 + client_id.encoded_length() + lwt.as_ref().map_or(0, |l| l.encoded_length()) +
                credentials.as_ref().map_or(0, |c| c.encoded_length())
            },
            ConnectAck{ .. } | PubAck{..} | PubRec{..} | PubRel{..} | PubComp{..} | UnsubAck {..} => 2,
            Publish{topic_name, message, packet_id, ..} => {
                topic_name.encoded_length() + packet_id.and(Some(2)).unwrap_or(0) + message.encoded_length()
            },
            Subscribe{ subscriptions, .. } => {
                2 + subscriptions.encoded_length()
            },
            SubAck{ return_codes, ..} => 2 + return_codes.encoded_length(),
            Unsubscribe{ topics, ..} => 2 + topics.encoded_length(),
            PingReq | PingResp | Disconnect => 0
        }
    }
    
    /// Determine the length of the encoded packet, if possible. Packets bigger then 256MB cannot be encoded, and
    /// will cause this method to return `None`.
    pub fn len(&self) -> Option<usize> {
        let payload_size = self.encoded_length();
        match payload_size {
            0...127 => Some(1),
            128...16_383 => Some(2),
            16_384...2_097_151 => Some(3),
            2_097_152 => Some(4),
            _ => None
        }.map(|b| 1 + b + payload_size)
    }
}
