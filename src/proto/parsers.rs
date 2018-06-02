use ::nom::{IResult, ErrorKind, Needed, be_u16, rest};
use ::enum_primitive::FromPrimitive;
use super::types::*;
use super::Headers;

/// Attempts to decode a variable length encoded number from the provided byte slice.
///
/// If a valid number is found, `Done(&[u8], usize)` will be returned. The `usize`
/// is the number thats been decoded, the `&[u8]` contains the rest of the slice.
///
/// If there are not enough bytes to fully decode the number, `Incomplete(Unknown)` will be
/// returned.
///
/// If it take more than 4 bytes to decode a number or another error occurs, an error will be
/// returned.
fn decode_vle(input: &[u8]) -> IResult<&[u8], usize> {
    let mut multiplier = 1;
    let mut value: usize = 0;
    let mut enc_byte_index: usize = 0;

    loop {
        if enc_byte_index > 3 {
            return IResult::Error(error_code!(ErrorKind::Custom(0)));
        }

        if let Some(enc_byte) = input.get(enc_byte_index) {
            value += (*enc_byte as usize & 127) * multiplier;
            multiplier *= 128;

            if (*enc_byte & 128) == 0 {
                break;
            }

            enc_byte_index += 1;
        } else {
            return IResult::Incomplete(Needed::Unknown)
        }
    }
    let (_, remain) = input.split_at(enc_byte_index+1);
    IResult::Done(remain, value)
}

#[cfg(test)]
mod vle_tests {
    use super::*;

    #[test]
    fn one_byte_vle() {
        let input = [0x19, 0x7F, 0x7F, 0x7F];
        assert_done_and_eq!(decode_vle(&input), 25);
    }

    #[test]
    fn two_byte_vle() {
        let input = [0xC1, 0x02, 0x7F, 0x7F];
        assert_done_and_eq!(decode_vle(&input), 321);
    }

    #[test]
    fn three_byte_vle() {
        let input = [0x94, 0x80, 0x01, 0x7F];
        assert_done_and_eq!(decode_vle(&input), 16_404);
    }

    #[test]
    fn four_byte_vle() {
        let input = [0xBC, 0x85, 0x80, 0x01];
        assert_done_and_eq!(decode_vle(&input), 2_097_852);
    }

    #[test]
    fn overflow_vle() {
        let input = [0x80, 0x80, 0x80, 0x80, 0x01];
        assert_error_and_eq!(decode_vle(&input), error_code!(ErrorKind::Custom(0)))
    }

    #[test]
    fn incomplete_vle() {
        let input = [0x80, 0x80];
        assert_needs!(decode_vle(&input), ?)
    }
}

fn error(input: &[u8], e: ErrorKind) -> IResult<&[u8], ()> {
    IResult::Error(error_code!(e))
}

named!(vle<&[u8], usize>, call!(decode_vle));
named!(bytes, length_bytes!(be_u16));

named!(packet_id, take!(2));
named!(conn_ack_flags, take!(1));
named!(conn_ret_code, take!(1));
named!(topic_name, length_bytes!(be_u16));
named!(packet_type<&[u8], PacketType>, map_opt!(
    bits!(take_bits!(u8, 4)), |b| PacketType::from_u8(b)));
named!(packet_flags<&[u8], PacketFlags>, map_opt!(
    bits!(take_bits!(u8, 4)), |b| PacketFlags::from_bits(b)));
named!(conn_ack_hdrs<&[u8], Headers>, do_parse!(
    flags: conn_ack_flags                 >>
    ret_code: conn_ret_code               >>
    ({
        let mut headers = Headers::new();
        headers.set_raw("connect_ack_flags", flags);
        headers.set_raw("connect_return_code", ret_code);
        headers
    })
));
named_args!(publish_hdrs(qos12: bool) <Headers>, do_parse!(
    topic: topic_name            >>
    pid: cond!(qos12, packet_id) >>
    ({
        let mut headers = Headers::new();
        headers.set_raw("topic_name", topic);
        if let Some(p) = pid {
            headers.set_raw("packet_id", p);
        }
        headers
    })
));
named!(pub_steps_hdrs<&[u8], Headers>, do_parse!(
    id: packet_id            >>
    ({
        let mut headers = Headers::new();
        headers.set_raw("packet_id", id);
        headers
    })
));
named!(sub_ack_hdrs<&[u8], Headers>, do_parse!(
    id: packet_id            >>
    ({
        let mut headers = Headers::new();
        headers.set_raw("packet_id", id);
        headers
    })
));
named!(unsub_ack_hdrs<&[u8], Headers>, do_parse!(
    id: packet_id            >>
    ({
        let mut headers = Headers::new();
        headers.set_raw("packet_id", id);
        headers
    })
));
named_args!(packet_headers(ty: PacketType, fl: PacketFlags) <Headers>, switch!(value!(ty),
    PacketType::ConnAck => call!(conn_ack_hdrs)                          |
    PacketType::Publish => call!(publish_hdrs, fl.contains(QOS1 & QOS2)) |
    PacketType::PubAck => call!(pub_steps_hdrs)                          |
    PacketType::PubRec   => call!(pub_steps_hdrs)                        |
    PacketType::PubRel   => call!(pub_steps_hdrs)                        |
    PacketType::PubComp  => call!(pub_steps_hdrs)                        |
    PacketType::SubAck   => call!(sub_ack_hdrs)                          |
    PacketType::UnsubAck => call!(unsub_ack_hdrs)                        |
    PacketType::PingResp => value!(Headers::new())                     |
    _                    => map!(call!(error, ErrorKind::Custom(2)), |_| Headers::new())
));

named_args!(packet_payload(ty: PacketType) <Payload>, switch!(value!(ty),
    PacketType::Publish => map!(rest, |b| Payload::from(b))                   |
    PacketType::SubAck  => map!(sub_ack_return_codes, |c| Payload::SubAck(c)) |
    _                   => value!(Payload::None)
));

named!(sub_ack_return_codes<&[u8], Vec<SubAckReturnCode>>, many1!(
    map_opt!(take!(1), |c: &[u8]| SubAckReturnCode::from_u8(c[0]))
));

fn count_bytes(input: &[u8]) -> IResult<&[u8], usize> {
    let size = input.len();
    IResult::Done(input, size)
}

named!(pub packet<&[u8], (PacketType, PacketFlags, Headers, Payload)>, do_parse!(
    ty: packet_type                              >>
    flags: packet_flags                          >>
    re_len: vle                                  >>
    bh_size: count_bytes                         >>
    headers: call!(packet_headers, ty, flags)    >>
    ah_size: count_bytes                         >>
    payload: length_value!(value!(re_len - (bh_size - ah_size)), call!(packet_payload, ty)) >>
    (ty, flags, headers, payload)
));

// #[cfg(test)]
// mod decode_tests {
//     use super::*;
//
//     #[test]
//     fn test_decode_conn_ack_hdrs() {
//         let input = [0x01, 0x00, 0x7F, 0x7F];
//         assert_done_and_eq!(conn_ack_hdrs(&input), vec![vec![0x01], vec![0x00]]);
//     }
//
//     #[test]
//     fn test_decode_publish_hdrs_qos0() {
//         let input = [0x00, 0x03, 0x61, 0x2F, 0x62, 0x00, 0x0A, 0x7F, 0x7F];
//         assert_done_and_eq!(publish_hdrs(&input, false), vec![vec![0x61, 0x2F, 0x62]]);
//     }
//
//     #[test]
//     fn test_decode_publish_hdrs_qos12() {
//         let input = [0x00, 0x03, 0x61, 0x2F, 0x62, 0x00, 0x0A, 0x7F, 0x7F];
//         assert_done_and_eq!(publish_hdrs(&input, true),
//             vec![vec![0x61, 0x2F, 0x62], vec![0x00, 0x0A]]);
//     }
//
//     #[test]
//     fn test_decode_pub_steps_hdrs() {
//         let input = [0x00, 0x0A, 0x7F, 0x7F];
//         assert_done_and_eq!(pub_steps_hdrs(&input), vec![vec![0x00, 0x0A]])
//     }
// }
