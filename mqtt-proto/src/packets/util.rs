use super::prelude::*;

/// Common header definition for packets that only have a packet id in their header.
pub struct PacketIdHeaders {
    pub packet_id: u16
}

impl PacketIdHeaders {
    pub fn new(packet_id: u16) -> PacketIdHeaders {
        PacketIdHeaders {
            packet_id
        }
    }
}

impl<'a> Headers<'a> for PacketIdHeaders {
    fn parse(input: &'a [u8], _: &PacketFlags) -> IResult<&'a [u8], PacketIdHeaders> {
        map!(input,be_u16, |p| PacketIdHeaders{ packet_id: p })
    }
}

impl<'a> Encodable for PacketIdHeaders {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u16_be(self.packet_id);
    }

    fn encoded_length(&self) -> usize { 2 }
}
