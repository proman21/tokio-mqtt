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

fn string_len_check(s: &str) -> Result<()> {
    if s.len() > 65535 {
        Err(Error::StringTooBig(s.len()))
    } else {
        Ok(())
    }
}

impl<'a> MqttPacket<'a> {
    pub fn packet_type(&self) -> PacketType {
        use self::MqttPacket::*;

        match self {
            Connect {.. } => PacketType::Connect,
            ConnAck { .. } => PacketType::ConnAck,
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
            Connect { .. } | ConnAck { .. } | PubAck{ .. } | PubRec{ .. } | PubRel{ .. } | PubComp{ .. } | 
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
        match packet(buf) {
            Ok(t) => Ok(Some(t)),
            Err(Err::Incomplete(_)) => Ok(None),
            Err(e) => Err(Error::ParseFailure(e.into_error_kind()))
        }
    }
    
    /// Attempt to encode the packet into a buffer.
    /// 
    /// If the packet is invalid or is too big to be encoded, an error will be raised.
    ///
    /// # Panics
    /// Panics if `out` does not have enough capacity to contain the packet. Use `len()` to determine the size of the
    /// encoded packet.
    pub fn encode<B: BufMut>(&self, out: &mut B) -> Result<()> {
        use self::MqttPacket::*;
        
        self.validate()?;
        
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
            ConnAck{ session_present, connect_return_code } => {
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
            ConnAck{ .. } | PubAck{..} | PubRec{..} | PubRel{..} | PubComp{..} | UnsubAck {..} => 2,
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
    
    /// Validates that the packet is correct according to the MQTT protocol rules.
    ///
    /// This function checks string lengths, as well as the Quality of Service rules for the Publish packet.
    pub fn validate(&self) -> Result<()> {
         use self::MqttPacket::*;
         
         match self {
             Connect{ client_id, lwt, credentials, .. } => {
                 string_len_check(client_id)?;
                 
                 if let Some(l) = lwt {
                      string_len_check(l.topic)?;
                 }
                 
                 if let Some(c) = credentials {
                      string_len_check(c.username)?;
                 }
             },
             ConnAck{session_present, connect_return_code} => {
                 if session_present & connect_return_code.is_err() {
                     return Err(Error::UnexpectedSessionPresent);
                 }
             }
             Publish{topic_name, packet_id, qos, dup, ..} => {
                string_len_check(topic_name)?;
                
                match qos {
                    QualityOfService::QoS0 => {
                        if *dup {
                            return Err(Error::InvalidDupFlag);
                        }
                        
                        if packet_id.is_some() {
                            return Err(Error::UnexpectedPublishPacketId)
                        }
                    },
                    QualityOfService::QoS1 | QualityOfService::QoS2 => {
                        if packet_id.is_none() {
                            return Err(Error::MissingPublishPacketId)
                        }
                    }
                }
             },
             Unsubscribe{ topics, ..} => {
                 for t in topics {
                      string_len_check(t)?;
                 }
             },
             _ => {}
         }
         
         Ok(())
    }
}
