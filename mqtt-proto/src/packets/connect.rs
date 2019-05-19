use super::prelude::*;

#[allow(dead_code)]
const SPEC_PROTO_NAME: &'static str = "MQTT";

pub fn connect<'a>(
    protocol_name: Option<&'a str>,
    protocol_level: ProtoLvl,
    clean_session: bool,
    keep_alive: u16,
    client_id: &'a str,
    lwt: Option<LWTMessage<&'a str, &'a [u8]>>,
    credentials: Option<Credentials<&'a str, &'a [u8]>>
) -> MqttPacket<'a> {
    let headers = ConnectHeaders {
        protocol_name: protocol_name.unwrap_or(SPEC_PROTO_NAME),
        protocol_level: protocol_level,
        connect_flags: {
            let mut flags = lwt.as_ref().map_or(ConnFlags::empty(), |f| f.connect_flags());
            flags.set(ConnFlags::CLEAN_SESS, clean_session);
            if let Some(c) = &credentials {
                flags.insert(ConnFlags::USERNAME);
                flags.set(ConnFlags::PASSWORD, c.password.is_some());
            }
            flags
        },
        keep_alive: keep_alive
    };

    let payload = ConnectPayload {
        client_id: client_id,
        lwt: lwt.as_ref().map(|l| (l.topic, l.message)),
        credentials: credentials
    };

    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::Connect {
            headers,
            payload
        }
    }
}

pub struct ConnectHeaders<'a> {
    pub protocol_name: &'a str,
    pub protocol_level: ProtoLvl,
    pub connect_flags: ConnFlags,
    pub keep_alive: u16
}

impl<'a> Headers<'a> for ConnectHeaders<'a> {
    fn parse(input: &'a [u8], _: &PacketFlags) -> IResult<&'a [u8], ConnectHeaders<'a>> {
        do_parse!(input,
            protocol_name: string >>
            protocol_level: call!(proto_lvl)    >>
            connect_flags: call!(conn_flags)    >>
            keep_alive: be_u16                  >>
            (ConnectHeaders {
                protocol_name,
                protocol_level,
                connect_flags,
                keep_alive
            })
        )
    }
}

impl<'a> Encodable for ConnectHeaders<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        out.put_u16_be(4);
        out.put_slice(b"MQTT");

        out.put_u8(self.protocol_level.as_u8());

        out.put_u8(self.connect_flags.bits());

        out.put_u16_be(self.keep_alive);
    }

    fn encoded_length(&self) -> usize {
        10
    }
}

pub struct ConnectPayload<'a> {
    pub client_id: &'a str,
    pub lwt: Option<(&'a str, &'a [u8])>,
    pub credentials: Option<Credentials<&'a str, &'a [u8]>>,
}

impl<'a> Payload<'a, ConnectHeaders<'a>> for ConnectPayload<'a> {
    fn parse(input: &'a [u8], _: &PacketFlags, headers: &ConnectHeaders) -> IResult<&'a [u8], ConnectPayload<'a>> {
        do_parse!(input,
            client_id: string >>
            lwt: cond!(
                headers.connect_flags.intersects(ConnFlags::WILL_FLAG),
                tuple!(string, length_bytes!(be_u16))
            ) >>
            username: cond!(
                headers.connect_flags.intersects(ConnFlags::USERNAME),
                string
            ) >>
            password: cond!(
                headers.connect_flags.intersects(ConnFlags::PASSWORD),
                length_bytes!(be_u16)
            ) >>
            (ConnectPayload {
                client_id,
                lwt,
                credentials: username.map(|u| Credentials {
                    username: u,
                    password
                })
            })
        )
    }
}

impl<'a> Encodable for ConnectPayload<'a> {
    fn encode<B: BufMut>(&self, out: &mut B) {
        self.client_id.encode(out);
        if let Some(lwt) = self.lwt {
            lwt.0.encode(out);
            lwt.1.encode(out);
        }
        if let Some(c) = &self.credentials {
            c.encode(out);
        }
    }

    fn encoded_length(&self) -> usize {
        self.client_id.encoded_length() +
        self.lwt.map_or(0, |(t, p)| t.encoded_length() + p.encoded_length()) +
        self.credentials.as_ref().map_or(0, |c| c.encoded_length())
    }
}
