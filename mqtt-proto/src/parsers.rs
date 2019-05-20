use std::collections::HashMap;
pub use ::nom::{IResult, Err, be_u16, be_u8};
use ::enum_primitive::FromPrimitive;
use super::types::*;
use super::MqttPacket;

pub type HeaderMap<'a> = HashMap<&'static str, &'a [u8]>;

/// Parses a single vle byte, returning the (value, multiplier, continuation) tuple
fn vle_byte(input: &[u8], (value, multiplier): (usize, usize)) -> IResult<&[u8], RecurseResult<(usize, usize), usize>> {
    map!(input, be_u8, |b| {
        let new_val = value + (b as usize & 127) * multiplier;
        if b & 128 == 0 {
            RecurseResult::Done(new_val)
        } else {
            RecurseResult::Continue((new_val, multiplier * 128))
        }
    })
}

enum RecurseResult<C, D> {
    Continue(C),
    Done(D)
}

/// Creates a recursive parser chain that passes the results of each invocation of the parser to
/// to the next invocation. The parser uses the `RecurseResult` enum to signal the stop of the
/// recursion.
///
/// ```ignore
/// recurse!(C, (I, C) -> IResult<I, RecurseResult<C, D>>) -> IResult<I, D>
/// ```
// macro_rules! recurse(
//     ($i:expr, $s:expr, $f:expr) => ({
//         use $crate::nom::ErrorKind;
//
//         let mut cont = $s;
//         let mut i = $i;
//         loop {
//             match add_return_error!(i, ErrorKind::Custom(666), call!($f, cont))? {
//                 (rest, RecurseResult::Continue(c)) => {
//                     i = rest;
//                     cont = c;
//                 },
//                 (rest, RecurseResult::Done(d)) => return Ok((rest, d)),
//             }
//         }
//     });
// );

/// Similar to `recurse!()`, except limits the number of recursions allowed to n inclusive.
///
/// ```ignore
/// recurse_m!(C, nb, (I, C) -> IResult<I, RecurseResult<C, D>>) -> IResult<I, D>
/// ```
macro_rules! recurse_m(
    ($i:expr, $s:expr, $m:expr, $f:expr) => ({
        use $crate::nom::ErrorKind;

        let mut cont = $s;
        let mut i = $i;
        let mut depth = $m;
        loop {
            match add_return_error!(i, ErrorKind::Custom(666), call!($f, cont))? {
                (rest, RecurseResult::Continue(c)) => {
                    if depth <= 1 {
                        return Err(Err::Error(error_position!(i, ErrorKind::Custom(666))));
                    }
                    depth -= 1;
                    i = rest;
                    cont = c;
                },
                (rest, RecurseResult::Done(d)) => return Ok((rest, d)),
            }
        }
    });
);

/// Attempts to decode a variable length encoded number from the provided byte slice.
named!(pub vle(&[u8]) -> usize, recurse_m!((0, 1), 5, vle_byte));

#[cfg(test)]
mod vle_tests {
    use super::*;

    #[test]
    fn one_byte_vle() {
        let input = [0x19, 0x7F, 0x7F, 0x7F];
        assert_done_and_eq!(vle(&input), 25);
    }

    #[test]
    fn two_byte_vle() {
        let input = [0xC1, 0x02, 0x7F, 0x7F];
        assert_done_and_eq!(vle(&input), 321);
    }

    #[test]
    fn three_byte_vle() {
        let input = [0x94, 0x80, 0x01, 0x7F];
        assert_done_and_eq!(vle(&input), 16_404);
    }

    #[test]
    fn four_byte_vle() {
        let input = [0xBC, 0x85, 0x80, 0x01];
        assert_done_and_eq!(vle(&input), 2_097_852);
    }

    #[test]
    fn overflow_vle() {
        let input = [0x80, 0x80, 0x80, 0x80, 0x01];
        assert_error_and_eq!(vle(&input), error_code!(ErrorKind::Custom(0)))
    }

    #[test]
    fn incomplete_vle() {
        let input = [0x80, 0x80];
        assert_needs!(vle(&input), ?)
    }
}

named!(pub conn_flags<&[u8], ConnFlags>, map_opt!(
    be_u8,
    |b| ConnFlags::from_bits(b)
));

named!(pub conn_ack_flags<&[u8], ConnAckFlags>, map_opt!(
    be_u8,
    |b| ConnAckFlags::from_bits(b)
));

named!(pub conn_ret_code(&[u8]) -> ConnRetCode, map_opt!(
    be_u8,
    |b| ConnRetCode::from_u8(b)
));

named!(pub string(&[u8]) -> &str, do_parse!(
    len: be_u16          >>
    utf8: take_str!(len) >>
    (utf8)
));

named!(pub packet_type<&[u8], PacketType>, map_opt!(
    bits!(take_bits!(u8, 4)),
    |b| PacketType::from_u8(b))
);

named!(pub packet_flags<&[u8], PacketFlags>, map_opt!(
    bits!(take_bits!(u8, 4)),
    |b| PacketFlags::from_bits(b))
);

named!(pub proto_lvl(&[u8]) -> ProtoLvl, map_opt!(
    be_u8,
    |b| ProtoLvl::from_u8(b)
));

named!(pub qos(&[u8]) -> QualityOfService, map_opt!(
    be_u8,
    |b| QualityOfService::from_u8(b)
));

named!(pub sub_ack_return_code(&[u8]) -> SubAckReturnCode, map_opt!(
    be_u8,
    |c| SubAckReturnCode::from_u8(c)
));

named!(pub connect_packet(&[u8]) -> MqttPacket, do_parse!(
    tag!("MQTT")              >>
    protocol_level: proto_lvl >>
    connect_flags: conn_flags >>
    keep_alive: be_u16        >>
    client_id: string         >>
    lwt: cond!(
        connect_flags.intersects(ConnFlags::WILL_FLAG),
        tuple!(string, length_bytes!(be_u16))
    ) >>
    username: cond!(
        connect_flags.intersects(ConnFlags::USERNAME),
        string
    ) >>
    password: cond!(
        connect_flags.intersects(ConnFlags::PASSWORD),
        length_bytes!(be_u16)
    ) >>
    (MqttPacket::Connect {
        protocol_level,
        clean_session: connect_flags.intersects(ConnFlags::CLEAN_SESS),
        keep_alive,
        client_id,
        lwt: lwt.map(|(t, p)| LWTMessage::from_flags(connect_flags, t, p)),
        credentials: username.map(|u| Credentials {
            username: u,
            password
        })
    })
));

named!(pub connect_ack_packet(&[u8]) -> MqttPacket, do_parse!(
    flags: conn_ack_flags >>
    connect_return_code: conn_ret_code >>
    (MqttPacket::ConnectAck {
        session_present: flags.intersects(ConnAckFlags::SP),
        connect_return_code
    })
));

named_args!(pub publish_packet(flags: PacketFlags) <MqttPacket>, do_parse!(
    topic_name: string >>
    packet_id: cond!(
        flags.intersects(PacketFlags::QOS1 | PacketFlags::QOS2),
        be_u16
    ) >>
    message: length_bytes!(be_u16) >>
    (MqttPacket::Publish {
        dup: flags.intersects(PacketFlags::DUP),
        qos: flags.qos(),
        retain: flags.intersects(PacketFlags::RET),
        topic_name,
        packet_id,
        message
    })
));

pub fn packet_id_header<'a, C>(input: &'a [u8], build: C) -> IResult<&'a [u8], MqttPacket<'a>>
    where C: Fn(u16) -> MqttPacket<'a>
{
    map!(input, be_u16, build)
}

named!(pub subscribe_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    subscriptions: many1!(map!(tuple!(string, qos), |(t, q)| SubscriptionTuple(t, q))) >>
    (MqttPacket::Subscribe {
        packet_id,
        subscriptions
    })
));

named!(pub sub_ack_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    return_codes: many1!(sub_ack_return_code) >>
    (MqttPacket::SubAck {
        packet_id,
        return_codes
    })
));

named!(pub unsubscribe_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    topics: many1!(string) >>
    (MqttPacket::Unsubscribe {
        packet_id,
        topics
    })
));

named!(pub packet(&[u8]) -> MqttPacket, do_parse!(
    ty: packet_type     >>
    flags: packet_flags >>
    packet: length_value!(vle, switch!(value!(ty),
        PacketType::Connect => call!(connect_packet) |
        PacketType::ConnectAck => call!(connect_ack_packet) |
        PacketType::Publish => call!(publish_packet, flags) |
        PacketType::PubAck => call!(packet_id_header, |id| MqttPacket::PubAck {packet_id: id}) |
        PacketType::PubRec => call!(packet_id_header, |id| MqttPacket::PubRec {packet_id: id}) |
        PacketType::PubRel =>  call!(packet_id_header, |id| MqttPacket::PubRel {packet_id: id}) |
        PacketType::PubComp =>  call!(packet_id_header, |id| MqttPacket::PubComp {packet_id: id}) |
        PacketType::Subscribe => call!(subscribe_packet) |
        PacketType::SubAck => call!(sub_ack_packet) |
        PacketType::Unsubscribe => call!(unsubscribe_packet) |
        PacketType::UnsubAck => call!(packet_id_header, |id| MqttPacket::UnsubAck {packet_id: id}) |
        PacketType::PingReq => value!(MqttPacket::PingReq) |
        PacketType::PingResp => value!(MqttPacket::PingResp) |
        PacketType::Disconnect => value!(MqttPacket::Disconnect)
    )) >>
    (packet)
));
