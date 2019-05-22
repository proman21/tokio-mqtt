pub use ::nom::{IResult, Err, be_u16, be_u8, ErrorKind, Needed};
use ::enum_primitive::FromPrimitive;
use super::types::*;
use super::MqttPacket;

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

/// Attempts to decode a variable length encoded number from the provided byte slice.
named!(pub vle(&[u8]) -> usize, recurse_m!((0, 1), 4, vle_byte));

#[cfg(test)]
mod vle_tests {
    use super::*;

    #[test]
    fn one_byte_vle() {
        let input = [0x19, 0x7F, 0x7F, 0x7F];
        assert_eq!(vle(&input), Ok((&[0x7F, 0x7F, 0x7F][..], 25)));
    }

    #[test]
    fn two_byte_vle() {
        let input = [0xC1, 0x02, 0x7F, 0x7F];
        assert_eq!(vle(&input), Ok((&[0x7F, 0x7F][..], 321)));
    }

    #[test]
    fn three_byte_vle() {
        let input = [0x94, 0x80, 0x01, 0x7F];
        assert_eq!(vle(&input), Ok((&[0x7F][..], 16_404)));
    }

    #[test]
    fn four_byte_vle() {
        let input = [0xBC, 0x85, 0x80, 0x01];
        assert_eq!(vle(&input), Ok((&[][..], 2_097_852)));
    }

    #[test]
    fn overflow_vle() {
        let input = [0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(vle(&input), Err(Err::Error(error_position!(&input[3..], ErrorKind::Custom(666)))));
    }

    #[test]
    fn incomplete_vle() {
        let input = [0x80, 0x80];
        assert_eq!(vle(&input), Err(Err::Incomplete(Needed::Size(1))));
    }
}

named!(conn_flags<&[u8], ConnFlags>, map_opt!(
    be_u8,
    |b| ConnFlags::from_bits(b)
));

#[cfg(test)]
mod conn_flags_tests {
    use super::*;
    
    #[test]
    fn parse_valid_conn_flags() {
        let input = [0xFE];
        assert_eq!(conn_flags(&input), Ok((&[][..], ConnFlags::all())));
    }
    
    #[test]
    fn parse_invalid_conn_flags() {
        let input = [0x01];
        assert_eq!(conn_flags(&input), Err(Err::Error(error_position!(&input[..], ErrorKind::MapOpt))));
    }
}

named!(conn_ack_flags<&[u8], ConnAckFlags>, map_opt!(
    be_u8,
    |b| ConnAckFlags::from_bits(b)
));

named!(conn_ret_code(&[u8]) -> ConnRetCode, map_opt!(
    be_u8,
    |b| ConnRetCode::from_u8(b)
));

named!(string(&[u8]) -> &str, do_parse!(
    len: be_u16          >>
    utf8: take_str!(len) >>
    (utf8)
));

named!(packet_type_flags(&[u8]) -> ((PacketType, PacketFlags)), bits!(do_parse!(
    ty: packet_type >>
    flags: packet_flags >>
    (ty, flags)
)));

named!(packet_type<(&[u8], usize), PacketType>, map_opt!(
    take_bits!(u8, 4),
    |b| PacketType::from_u8(b)
));

named!(packet_flags<(&[u8], usize), PacketFlags>, map_opt!(
    take_bits!(u8, 4),
    |b| PacketFlags::from_bits(b)
));

named!(proto_lvl(&[u8]) -> ProtoLvl, map_opt!(
    be_u8,
    |b| ProtoLvl::from_u8(b)
));

named!(qos(&[u8]) -> QualityOfService, map_opt!(
    be_u8,
    |b| QualityOfService::from_u8(b)
));

named!(sub_ack_return_code(&[u8]) -> SubAckReturnCode, map_opt!(
    be_u8,
    |c| SubAckReturnCode::from_u8(c)
));

named!(connect_packet(&[u8]) -> MqttPacket, do_parse!(
    length_value!(be_u16, tag!("MQTT")) >>
    protocol_level: proto_lvl           >>
    connect_flags: conn_flags           >>
    keep_alive: be_u16                  >>
    client_id: string                   >>
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

named!(conn_ack_packet(&[u8]) -> MqttPacket, do_parse!(
    flags: conn_ack_flags >>
    connect_return_code: conn_ret_code >>
    (MqttPacket::ConnAck {
        session_present: flags.intersects(ConnAckFlags::SP),
        connect_return_code
    })
));

named_args!(publish_packet(flags: PacketFlags) <MqttPacket>, do_parse!(
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

fn packet_id_header<'a, C>(input: &'a [u8], build: C) -> IResult<&'a [u8], MqttPacket<'a>>
    where C: Fn(u16) -> MqttPacket<'a>
{
    map!(input, be_u16, build)
}

named!(subscribe_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    subscriptions: many1!(map!(tuple!(string, qos), |(t, q)| SubscriptionTuple(t, q))) >>
    (MqttPacket::Subscribe {
        packet_id,
        subscriptions
    })
));

named!(sub_ack_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    return_codes: many1!(sub_ack_return_code) >>
    (MqttPacket::SubAck {
        packet_id,
        return_codes
    })
));

named!(unsubscribe_packet(&[u8]) -> MqttPacket, do_parse!(
    packet_id: be_u16 >>
    topics: many1!(string) >>
    (MqttPacket::Unsubscribe {
        packet_id,
        topics
    })
));

named!(pub(crate) packet(&[u8]) -> MqttPacket, do_parse!(
    first: packet_type_flags >> // This will change to tuple destructuring when it is available in nom. (Geal/nom#869)
    packet: length_value!(vle, switch!(value!(first.0),
        PacketType::Connect => cond_reduce!(first.1.contains(PacketFlags::empty()), connect_packet) |
        PacketType::ConnAck => cond_reduce!(first.1.contains(PacketFlags::empty()), conn_ack_packet) |
        PacketType::Publish => call!(publish_packet, first.1) |
        PacketType::PubAck => cond_reduce!(first.1.contains(PacketFlags::empty()),
            call!(packet_id_header, |id| MqttPacket::PubAck {packet_id: id})) |
        PacketType::PubRec => cond_reduce!(first.1.contains(PacketFlags::empty()),
            call!(packet_id_header, |id| MqttPacket::PubRec {packet_id: id})) |
        PacketType::PubRel =>  cond_reduce!(first.1.contains(PacketFlags::empty()),
            call!(packet_id_header, |id| MqttPacket::PubRel {packet_id: id})) |
        PacketType::PubComp =>  cond_reduce!(first.1.contains(PacketFlags::empty()),
            call!(packet_id_header, |id| MqttPacket::PubComp {packet_id: id})) |
        PacketType::Subscribe => cond_reduce!(first.1.contains(PacketFlags::QOS1), subscribe_packet) |
        PacketType::SubAck => cond_reduce!(first.1.contains(PacketFlags::empty()), sub_ack_packet) |
        PacketType::Unsubscribe => cond_reduce!(first.1.contains(PacketFlags::QOS1), unsubscribe_packet) |
        PacketType::UnsubAck => cond_reduce!(first.1.contains(PacketFlags::empty()),
            call!(packet_id_header, |id| MqttPacket::UnsubAck {packet_id: id})) |
        PacketType::PingReq => cond_reduce!(first.1.contains(PacketFlags::empty()), value!(MqttPacket::PingReq)) |
        PacketType::PingResp => cond_reduce!(first.1.contains(PacketFlags::empty()), value!(MqttPacket::PingResp)) |
        PacketType::Disconnect => cond_reduce!(first.1.contains(PacketFlags::empty()), value!(MqttPacket::Disconnect))
    )) >>
    (packet)
));
