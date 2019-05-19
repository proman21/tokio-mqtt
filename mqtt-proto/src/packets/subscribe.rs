use super::prelude::*;

pub fn subscribe<'a>(
    packet_id: u16,
    subscriptions: Vec<(&'a str, QualityOfService)>
) -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::QOS1,
        contents: Contents::Subscribe {
            headers: PacketIdHeaders::new(packet_id),
            payload: SubscribePayload::new(subscriptions)
        }
    }
}

pub struct SubscribePayload<'a> {
    subscriptions: Vec<(&'a str, QualityOfService)>
}

impl<'a> SubscribePayload<'a> {
    pub fn new(subscriptions: Vec<(&'a str, QualityOfService)>) -> SubscribePayload<'a> {
        SubscribePayload {
            subscriptions
        }
    }
}

impl<'a> Payload<'a, PacketIdHeaders> for SubscribePayload<'a> {
    fn parse(input: &'a [u8], _: &PacketFlags, _: &PacketIdHeaders)
        -> IResult<&'a [u8], SubscribePayload<'a>>
    {
        map!(input, many1!(tuple!(string, qos)), |s| SubscribePayload{ subscriptions: s })
    }
}

impl<'a> Encodable for SubscribePayload<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        for (filter, qos) in &self.subscriptions {
            filter.encode(out);
            qos.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        (&self.subscriptions).into_iter()
            .fold(0, |acc, (f, q)| acc + f.encoded_length() + q.encoded_length())
    }
}
