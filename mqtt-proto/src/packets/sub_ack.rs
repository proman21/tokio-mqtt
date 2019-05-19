use super::prelude::*;

pub fn sub_ack<'a>(packet_id: u16, return_codes: Vec<SubAckReturnCode>) -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::SubAck {
            headers: PacketIdHeaders::new(packet_id),
            payload: SubAckPayload::new(return_codes)
        }
    }
}

pub struct SubAckPayload {
    return_codes: Vec<SubAckReturnCode>
}

impl<'a> SubAckPayload {
    pub fn new(return_codes: Vec<SubAckReturnCode>) -> SubAckPayload {
        SubAckPayload {
            return_codes
        }
    }
}

impl<'a> Payload<'a, PacketIdHeaders> for SubAckPayload {
    fn parse(input: &'a [u8], _: &PacketFlags, _: &PacketIdHeaders)
        -> IResult<&'a [u8], SubAckPayload>
    {
        map!(input, many1!(sub_ack_return_code), |c| SubAckPayload{ return_codes: c })
    }
}

impl Encodable for SubAckPayload {
    fn encode<B: BufMut>(&self, out: &mut B) {
        for code in &self.return_codes {
            code.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        (&self.return_codes).into_iter()
            .fold(0, |acc, c| acc + c.encoded_length())
    }
}
