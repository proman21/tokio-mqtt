use std::collections::HashMap;
pub use ::nom::{IResult, Err, be_u16, be_u8};
use ::enum_primitive::FromPrimitive;
use super::types::*;

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

named!(pub fixed_header(&[u8]) -> (PacketType, PacketFlags, usize), do_parse!(
    ty: packet_type     >>
    flags: packet_flags >>
    re_len: vle         >>
    (ty, flags, re_len)
));
