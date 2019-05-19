use super::prelude::*;

pub fn publish_qos0<'a, T: 'a + AsRef<str>, P: 'a + AsRef<[u8]>>(
    topic: &'a T,
    retain: bool,
    payload: &'a P
) -> MqttPacket<'a> {
    publish(topic, false, QualityOfService::QoS0, retain, None, payload)
}

pub fn publish_qos1<'a, T: 'a + AsRef<str>, P: 'a + AsRef<[u8]>>(
    topic: &'a T,
    dup: bool,
    retain: bool,
    packet_id: u16,
    payload: &'a P
) -> MqttPacket<'a> {
    publish(topic, dup, QualityOfService::QoS1, retain, Some(packet_id), payload)
}

pub fn publish_qos2<'a, T: 'a + AsRef<str>, P: 'a + AsRef<[u8]>>(
    topic: &'a T,
    dup: bool,
    retain: bool,
    packet_id: u16,
    payload: &'a P
) -> MqttPacket<'a> {
    publish(topic, dup, QualityOfService::QoS2, retain, Some(packet_id), payload)
}

fn publish<'a, T: 'a + AsRef<str>, P: 'a + AsRef<[u8]>>(
    topic: &'a T,
    dup: bool,
    qos: QualityOfService,
    retain: bool,
    packet_id: Option<u16>,
    payload: &'a P
) -> MqttPacket<'a> {
    let headers = PublishHeaders {
        topic_name: topic.as_ref(),
        packet_id
    };

    let payload = PublishPayload(payload.as_ref());

    let mut flags: PacketFlags = qos.into();
    flags.set(PacketFlags::DUP, dup);
    flags.set(PacketFlags::RET, retain);

    MqttPacket {
        flags,
        contents: Contents::Publish {
            headers,
            payload
        }
    }
}

fn pub_resp<'a, C>(ty: C, packet_id: u16) -> MqttPacket<'a>
    where C: FnOnce(PacketIdHeaders)
        -> Contents<'a>
{
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: ty(PacketIdHeaders::new(packet_id))
    }
}

pub fn pub_ack<'a>(packet_id: u16) -> MqttPacket<'a> {
    pub_resp(|headers| Contents::PubAck(headers), packet_id)
}

pub fn pub_rec<'a>(packet_id: u16) -> MqttPacket<'a> {
    pub_resp(|headers| Contents::PubRec(headers), packet_id)
}

pub fn pub_rel<'a>(packet_id: u16) -> MqttPacket<'a> {
    pub_resp(|headers| Contents::PubRel(headers), packet_id)
}

pub fn pub_comp<'a>(packet_id: u16) -> MqttPacket<'a> {
    pub_resp(|headers| Contents::PubComp(headers), packet_id)
}

pub struct PublishHeaders<'a> {
    pub topic_name: &'a str,
    pub packet_id: Option<u16>
}

impl<'a> Headers<'a> for PublishHeaders<'a> {
    fn parse(input: &'a [u8], flags: &PacketFlags) -> IResult<&'a [u8], PublishHeaders<'a>> {
        do_parse!(input,
            topic_name: string >>
            packet_id: cond!(
                flags.intersects(PacketFlags::QOS1 | PacketFlags::QOS2),
                be_u16
            ) >>
            (PublishHeaders {
                topic_name,
                packet_id
            })
        )
    }
}

impl<'a> Encodable for PublishHeaders<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.topic_name.encode(out);
        if let Some(id) = self.packet_id {
            out.put_u16_be(id);
        }
    }

    fn encoded_length(&self) -> usize {
        self.topic_name.encoded_length() + self.packet_id.and(Some(2)).unwrap_or(0)
    }
}

pub struct PublishPayload<'a>(&'a [u8]);

impl<'a> Payload<'a, PublishHeaders<'a>> for PublishPayload<'a> {
    fn parse(input: &'a [u8], _: &PacketFlags, _: &PublishHeaders<'a>) -> IResult<&'a [u8], PublishPayload<'a>> {
        Ok((&[], PublishPayload(input)))
    }
}

impl<'a> Encodable for PublishPayload<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_slice(self.0);
    }

    fn encoded_length(&self) -> usize {
        self.0.len()
    }
}
