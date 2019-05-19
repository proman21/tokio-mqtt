use super::prelude::*;

pub fn connect_ack<'a>(
    session_present: bool,
    connect_return_code: ConnRetCode,
) -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::ConnectAck(ConnectAckHeaders {
            connect_ack_flags: if session_present {
                ConnAckFlags::SP
            } else {
                ConnAckFlags::empty()
            },
            connect_return_code
        })
    }
}

pub struct ConnectAckHeaders {
    pub connect_ack_flags: ConnAckFlags,
    pub connect_return_code: ConnRetCode,
}

impl<'a> Headers<'a> for ConnectAckHeaders {
    fn parse(input: &'a [u8], _: &PacketFlags) -> IResult<&'a [u8], ConnectAckHeaders> {
        do_parse!(input,
            connect_ack_flags: conn_ack_flags >>
            connect_return_code: conn_ret_code >>
            (ConnectAckHeaders {
                connect_ack_flags,
                connect_return_code
            })
        )
    }
}

impl Encodable for ConnectAckHeaders {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u8(self.connect_ack_flags.bits());
        out.put_u8(self.connect_return_code.into())
    }

    fn encoded_length(&self) -> usize { 2 }
}
