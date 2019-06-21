use bytes::BufMut;
use nom::Err;

use super::MqttPacket;
use errors::*;
use parsers::*;
use types::*;

fn encode_vle<B: BufMut>(num: usize, out: &mut B) -> Result<(), Error<'static>> {
    let mut val: usize = num;

    ensure!(num <= 268_435_455, PacketTooBig { encoded_size: num });

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
    /// Returns the type of this MQTT Control packet.
    ///
    /// It may be easier to use the `MqttPacket` type in a match statement so that the identification and use of the
    /// packet contents is easier. This method is provided for convenience.
    /// 
    /// # Examples
    /// ```
    /// # use mqtt3_proto::{MqttPacket, PacketType};
    /// let packet1 = MqttPacket::PubAck{ packet_id: 1 };
    /// assert_eq!(packet1.packet_type(), PacketType::PubAck);
    /// ```
    pub fn packet_type(&self) -> PacketType {
        use self::MqttPacket::*;

        match self {
            Connect { .. } => PacketType::Connect,
            ConnAck { .. } => PacketType::ConnAck,
            Publish { .. } => PacketType::Publish,
            PubAck { .. } => PacketType::PubAck,
            PubRec { .. } => PacketType::PubRec,
            PubRel { .. } => PacketType::PubRel,
            PubComp { .. } => PacketType::PubComp,
            Subscribe { .. } => PacketType::Subscribe,
            SubAck { .. } => PacketType::SubAck,
            Unsubscribe { .. } => PacketType::Unsubscribe,
            UnsubAck { .. } => PacketType::UnsubAck,
            PingReq => PacketType::PingReq,
            PingResp => PacketType::PingResp,
            Disconnect => PacketType::Disconnect,
        }
    }

    fn packet_flags(&self) -> PacketFlags {
        use MqttPacket::*;

        match self {
            Publish {
                pub_type, retain, ..
            } => {
                let mut flags = pub_type.packet_flags();
                flags.set(PacketFlags::RET, *retain);
                flags
            }
            Subscribe { .. } | Unsubscribe { .. } => PacketFlags::QOS1,
            _ => PacketFlags::empty(),
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
    /// If an problem occurs while decoding the bytes, an error will be returned.
    /// 
    /// # Examples
    /// ```
    /// # use mqtt3_proto::{MqttPacket, Error};
    /// let buffer: Vec<u8> = vec![0b10110000, 2, 0, 1];
    /// assert_eq!(MqttPacket::from_buf(&buffer), Ok(Some((&[][..], MqttPacket::UnsubAck{ packet_id: 1}))));
    /// ```
    pub fn from_buf<B: AsRef<[u8]>>(
        buf: &'a B,
    ) -> Result<Option<(&'a [u8], MqttPacket<'a>)>, Error<'a>> {
        MqttPacket::from_slice(buf.as_ref())
    }

    // Non monomorphized code path for parsing to reduce library size.
    fn from_slice(buf: &'a [u8]) -> Result<Option<(&'a [u8], MqttPacket<'a>)>, Error<'a>> {
        match packet(buf) {
            Ok(t) => Ok(Some(t)),
            Err(Err::Incomplete(_)) => Ok(None),
            Err(Err::Error(e)) | Err(Err::Failure(e)) => Err(e.into_inner().unwrap()),
        }
    }

    /// Attempt to encode the packet into a buffer.
    ///
    /// If the packet is invalid or is too big to be encoded, an error will be raised.
    ///
    /// # Panics
    /// Panics if `out` does not have enough capacity to contain the packet. Use `len()` to determine the size of the
    /// encoded packet.
    /// 
    /// # Examples
    /// ```
    /// ```
    pub fn encode<B: BufMut>(&self, out: &mut B) -> Result<(), Error<'a>> {
        use self::MqttPacket::*;

        self.validate()?;

        out.put_u8(((self.packet_type() as u8) << 4) + self.packet_flags().bits());

        encode_vle(self.encoded_length(), out)?;
        match self {
            Connect {
                protocol_level,
                clean_session,
                keep_alive,
                client_id,
                lwt,
                credentials,
            } => {
                MqttString::new_unchecked("MQTT").encode(out);

                out.put_u8(*protocol_level as u8);

                let mut flags = lwt
                    .as_ref()
                    .map_or(ConnFlags::empty(), |f| f.connect_flags());
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
            }
            ConnAck { result } => match result {
                Ok(flags) => {
                    out.put_u8(flags.bits());
                    out.put_u8(0);
                }
                Err(e) => {
                    out.put_u8(ConnAckFlags::empty().bits());
                    out.put_u8(*e as u8);
                }
            },
            Publish { topic_name, pub_type, message, ..} => {
                topic_name.encode(out);
                if let Some(id) = pub_type.packet_id() {
                    out.put_u16_be(id);
                }
                message.encode(out);
            }
            PubAck { packet_id }
            | PubRec { packet_id }
            | PubRel { packet_id }
            | PubComp { packet_id }
            | UnsubAck { packet_id } => {
                out.put_u16_be(*packet_id);
            }
            Subscribe {
                packet_id,
                subscriptions,
            } => {
                out.put_u16_be(*packet_id);
                subscriptions.encode(out);
            }
            SubAck { packet_id, results } => {
                out.put_u16_be(*packet_id);
                results.encode(out);
            }
            Unsubscribe { packet_id, topics } => {
                out.put_u16_be(*packet_id);
                topics.encode(out);
            }
            PingReq | PingResp | Disconnect => {}
        }

        Ok(())
    }

    fn encoded_length(&self) -> usize {
        use self::MqttPacket::*;

        match self {
            Connect {
                client_id,
                lwt,
                credentials,
                ..
            } => {
                10 + client_id.encoded_length()
                    + lwt.as_ref().map_or(0, |l| l.encoded_length())
                    + credentials.as_ref().map_or(0, |c| c.encoded_length())
            }
            ConnAck { .. }
            | PubAck { .. }
            | PubRec { .. }
            | PubRel { .. }
            | PubComp { .. }
            | UnsubAck { .. } => 2,
            Publish {
                topic_name,
                message,
                pub_type,
                ..
            } => {
                topic_name.encoded_length()
                    + pub_type.packet_id().and(Some(2)).unwrap_or(0)
                    + message.encoded_length()
            }
            Subscribe { subscriptions, .. } => 2 + subscriptions.encoded_length(),
            SubAck { results, .. } => 2 + results.encoded_length(),
            Unsubscribe { topics, .. } => 2 + topics.encoded_length(),
            PingReq | PingResp | Disconnect => 0,
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
            _ => None,
        }
        .map(|b| 1 + b + payload_size)
    }

    /// Validates that the packet is correct according to the MQTT protocol rules.
    ///
    /// This function checks that Subscribe, Unsubscribe, and SubAck payloads are not empty.
    pub fn validate(&self) -> Result<(), Error<'a>> {
        use self::MqttPacket::*;

        match self {
            Unsubscribe { topics, .. } => {
                ensure!(!topics.is_empty(), MissingPayload);
            }
            Subscribe { subscriptions, .. } => {
                ensure!(!subscriptions.is_empty(), MissingPayload);
            }
            SubAck { results, .. } => {
                ensure!(!results.is_empty(), MissingPayload);
            }
            _ => {}
        }

        Ok(())
    }
}
