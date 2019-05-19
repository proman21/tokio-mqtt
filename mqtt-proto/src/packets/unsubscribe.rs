use super::prelude::*;

pub fn unsubscribe<'a>(packet_id: u16, topics: Vec<&'a str>) -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::QOS1,
        contents: Contents::Unsubscribe {
            headers: PacketIdHeaders::new(packet_id),
            payload: UnsubscribePayload::new(topics)
        }
    }
}

pub fn unsub_ack<'a>(packet_id: u16) -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::UnsubAck (
            PacketIdHeaders::new(packet_id)
        )
    }
}

pub struct UnsubscribePayload<'a> {
    topics: Vec<&'a str>
}

impl<'a> UnsubscribePayload<'a> {
    pub fn new(topics: Vec<&'a str>) -> UnsubscribePayload {
        UnsubscribePayload {
            topics
        }
    }
}

impl<'a> Payload<'a, PacketIdHeaders> for UnsubscribePayload<'a> {
    fn parse(input: &'a [u8], _: &PacketFlags, _: &PacketIdHeaders)
        -> IResult<&'a [u8], UnsubscribePayload<'a>>
    {
        map!(input, many1!(string), |t| UnsubscribePayload{ topics: t })
    }
}

impl<'a> Encodable for UnsubscribePayload<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        for filter in &self.topics {
            filter.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        (&self.topics).into_iter()
            .fold(0, |acc, t| acc + t.encoded_length())
    }
}
