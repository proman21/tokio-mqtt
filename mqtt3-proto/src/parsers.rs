use std::convert::TryFrom;
use std::str;

use nom::bits::{bits, streaming::take as take_bits};
use nom::bytes::streaming::take;
use nom::combinator::{complete, cond, flat_map, map, map_parser};
use nom::error::*;
use nom::multi::many1;
use nom::number::streaming::{be_u16, be_u8};
use nom::sequence::tuple;
use nom::{Err, IResult, ErrorConvert};

use super::errors::Error;
use super::types::*;
use super::MqttPacket;

#[derive(PartialEq, Debug)]
pub(crate) struct ParserError<'a>(Option<Error<'a>>);

impl<'a> ParserError<'a> {
    pub fn some(e: Error<'a>) -> ParserError<'a> {
        ParserError(Some(e))
    }

    pub fn none() -> ParserError<'a> {
        ParserError(None)
    }

    pub fn into_inner(self) -> Option<Error<'a>> {
        self.0
    }
}

impl<'a> From<Option<Error<'a>>> for ParserError<'a> {
    fn from(value: Option<Error<'a>>) -> ParserError<'a> {
        ParserError(value)
    }
}

impl<'a> ParseError<&'a [u8]> for ParserError<'a> {
    fn from_error_kind(_: &'a [u8], _: ErrorKind) -> ParserError<'a> {
        ParserError::none()
    }

    fn append(_: &'a [u8], _: ErrorKind, other: ParserError<'a>) -> ParserError<'a> {
        other
    }
}

impl<'a> ParseError<(&'a [u8], usize)> for ParserError<'a> {
    fn from_error_kind(_: (&'a [u8], usize), _: ErrorKind) -> ParserError<'a> {
        ParserError::none()
    }

    fn append(_: (&'a [u8], usize), _: ErrorKind, other: ParserError<'a>) -> ParserError<'a> {
        other
    }
}

impl<'a> ErrorConvert<ParserError<'a>> for ParserError<'a> {
    fn convert(self) -> ParserError<'a> {
        self
    }
}

type ParserResult<'a, I, O> = IResult<I, O, ParserError<'a>>;

fn map_res_err<I: Clone, O1, O2, E: ParseError<I>, F, G>(
    first: F,
    second: G,
) -> impl Fn(I) -> IResult<I, O2, E>
where
    F: Fn(I) -> IResult<I, O1, E>,
    G: Fn(O1) -> Result<O2, E>,
{
    move |input: I| {
        let i = input.clone();
        let (input, o1) = first(input)?;
        match second(o1) {
            Ok(o2) => Ok((input, o2)),
            Err(e) => Err(Err::Error(E::append(i, ErrorKind::MapRes, e))),
        }
    }
}

/// Attempts to decode a variable length encoded number from the provided byte slice.
fn vle(input: &[u8]) -> ParserResult<&[u8], usize> {
    let mut value = 0;
    let mut multiplier = 1;
    let mut i = input;

    loop {
        let (rest, b) = be_u8(i)?;

        value += (b as usize & 127) * multiplier;

        if multiplier > 128 * 128 * 128 {
            return Err(Err::Failure(ParserError::some(Error::VleOverflow)));
        }

        multiplier *= 128;
        i = rest;

        if b & 128 == 0 {
            break;
        }
    }

    Ok((i, value))
}

#[cfg(test)]
mod vle_tests {
    use super::{vle, Err, Error, ParserError};
    use nom::Needed;

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
        assert_eq!(
            vle(&input),
            Err(Err::Failure(ParserError::some(Error::VleOverflow)))
        );
    }

    #[test]
    fn incomplete_vle() {
        let input = [0x80, 0x80];
        assert_eq!(vle(&input), Err(Err::Incomplete(Needed::Size(1))));
    }
}

fn add_error<'a, I, O, F, G>(parser: F, op: G) -> impl Fn(I) -> ParserResult<'a, I, O>
where
    F: Fn(I) -> ParserResult<'a, I, O>,
    G: Fn() -> Error<'a>,
{
    move |input: I| match parser(input) {
        Ok(o) => Ok(o),
        Err(Err::Incomplete(n)) => Err(Err::Incomplete(n)),
        Err(Err::Error(e)) => Err(Err::Error(e.into_inner().or(Some(op())).into())),
        Err(Err::Failure(e)) => Err(Err::Failure(e.into_inner().or(Some(op())).into())),
    }
}

fn mqtt_string<'a>(input: &'a [u8]) -> ParserResult<'a, &'a [u8], MqttString<'a>> {
    map_res_err(
        map_res_err(flat_map(be_u16, take), |b| {
            str::from_utf8(b).map_err(|e| {
                ParserError(Some(Error::StringNotUtf8 {
                    input: b.clone(),
                    source: e,
                }))
            })
        }),
        |s| MqttString::new(s).map_err(|e| ParserError::some(e)),
    )(input)
}

#[cfg(test)]
mod mqtt_string_tests {
    use super::{mqtt_string, Err, Error, MqttString, ParserError};
    use std::str;
    use nom::Needed;

    #[test]
    fn normal_strings() {
        let one = [0, 4, 77, 81, 84, 84];
        assert_eq!(
            mqtt_string(&one),
            Ok((&[][..], MqttString::new_unchecked("MQTT")))
        );
    }

    #[test]
    fn null_terminated_strings() {
        let null_terminated = [0x0, 0xC, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x77, 0x6F, 0x72, 0x6C, 0x64, 0x0,];
        assert_eq!(
            mqtt_string(&null_terminated),
            Err(Err::Error(ParserError::some(Error::InvalidString{ string: "hello world\0"})))
        );
    }

    #[test]
    fn not_utf8() {
        let not_utf8 = [0x0, 0xA, 0x7f, 0xaa, 0x72, 0x23, 0x7f, 0x0d, 0x47, 0xc9, 0x45, 0x49,];
        let utf8_err = str::from_utf8(&not_utf8[2..]).unwrap_err();
        assert_eq!(
            mqtt_string(&not_utf8),
            Err(Err::Error(ParserError::some(Error::StringNotUtf8{ input: &not_utf8[2..], source: utf8_err })))
        );
    }

    #[test]
    fn missing_input() {
        let not_enough = [0x0, 0xB, 0x68, 0x65, 0x6C, 0x6C,];
        assert_eq!(mqtt_string(&not_enough), Err(Err::Incomplete(Needed::Size(0xB))))
    }
}

fn connect_packet<'a>(input: &'a [u8]) -> ParserResult<'a, &'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, _) = map_res_err(mqtt_string, |name| {
        if name.as_ref() == "MQTT" {
            Ok(name)
        } else {
            Err(ParserError::some(Error::InvalidProtocolName { name: name }))
        }
    })(input)?;

    let (r2, protocol_level) = map_res_err(be_u8, |pl| {
        ProtoLvl::try_from(pl).map_err(|e| ParserError::some(e))
    })(r1)?;

    let (r3, connect_flags) = map_res_err(be_u8, |cf| {
        ConnFlags::try_from(cf).map_err(|e| ParserError::some(e))
    })(r2)?;

    let (r4, keep_alive) = be_u16(r3)?;

    // Payload
    let (r5, client_id) = mqtt_string(r4)?;

    let (r6, lwt) = cond(
        connect_flags.has_lwt(),
        tuple((mqtt_string, flat_map(be_u16, take))),
    )(r5)?;

    let (r7, username) = cond(connect_flags.has_username(), mqtt_string)(r6)?;

    let (r8, password) = cond(connect_flags.has_password(), flat_map(be_u16, take))(r7)?;

    Ok((
        r8,
        MqttPacket::Connect {
            protocol_level,
            clean_session: connect_flags.is_clean(),
            keep_alive,
            client_id,
            lwt: lwt.map(|(t, p)| LWTMessage::from_flags(connect_flags, t, p)),
            credentials: username.map(|u| Credentials {
                username: u,
                password,
            }),
        },
    ))
}

fn conn_ack_packet<'a>(input: &'a [u8]) -> ParserResult<'a, &'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, flags) = map_res_err(be_u8, |b| {
        ConnAckFlags::try_from(b).map_err(|e| ParserError::some(e))
    })(input)?;

    let (r2, result) = map_res_err(be_u8, |c| {
        connect_result_from_u8(c).map_err(|e| ParserError::some(e))
    })(r1)?;

    Ok((
        r2,
        MqttPacket::ConnAck {
            result: result.and(Ok(flags)),
        },
    ))
}

fn publish_packet<'a>(
    input: &'a [u8],
    flags: PacketFlags,
) -> ParserResult<'a, &'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, topic_name) = mqtt_string(input)?;

    let (r2, packet_id) = cond(
        flags.intersects(PacketFlags::QOS1 | PacketFlags::QOS2),
        be_u16,
    )(r1)?;

    // Payload
    let (r3, message) = flat_map(be_u16, take)(r2)?;

    let pub_type =
        PublishType::new(flags, packet_id).map_err(|e| Err::Error(ParserError::some(e)))?;

    Ok((
        r3,
        MqttPacket::Publish {
            pub_type,
            retain: flags.is_retain(),
            topic_name,
            message,
        },
    ))
}

fn packet_id_header<'a, C>(input: &'a [u8], build: C) -> ParserResult<&'a [u8], MqttPacket<'a>>
where
    C: Fn(u16) -> MqttPacket<'a>,
{
    map(be_u16, build)(input)
}

fn subscribe_packet<'a>(input: &'a [u8]) -> ParserResult<'a, &'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, packet_id) = be_u16(input)?;

    // Payload
    let (r2, subscriptions) = add_error(
        many1(map(
            tuple((
                mqtt_string,
                map_res_err(be_u8, |b| {
                    QualityOfService::try_from(b).map_err(|e| ParserError::some(e))
                }),
            )),
            |(t, q)| SubscriptionTuple(t, q),
        )),
        || Error::MissingPayload,
    )(r1)?;

    Ok((
        r2,
        MqttPacket::Subscribe {
            packet_id,
            subscriptions,
        },
    ))
}

fn sub_ack_packet<'a>(input: &'a [u8]) -> ParserResult<&'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, packet_id) = be_u16(input)?;

    // Payload
    let (r2, results) = add_error(
        many1(map_res_err(be_u8, |code| {
            SubAckReturnCode::try_from(code).map_err(|e| ParserError::some(e))
        })),
        || Error::MissingPayload,
    )(r1)?;

    Ok((r2, MqttPacket::SubAck { packet_id, results }))
}

fn unsubscribe_packet<'a>(input: &'a [u8]) -> ParserResult<&'a [u8], MqttPacket<'a>> {
    // Headers
    let (r1, packet_id) = be_u16(input)?;

    // Payload
    let (r2, topics) = add_error(many1(mqtt_string), || Error::MissingPayload)(r1)?;

    Ok((r2, MqttPacket::Unsubscribe { packet_id, topics }))
}

fn packet_body<'a>(
    ty: PacketType,
    flags: PacketFlags,
) -> impl Fn(&'a [u8]) -> ParserResult<&'a [u8], MqttPacket<'a>> {
    move |input: &[u8]| match ty {
        PacketType::Connect => connect_packet(input),
        PacketType::ConnAck => conn_ack_packet(input),
        PacketType::Publish => publish_packet(input, flags),
        PacketType::PubAck => packet_id_header(input, |id| MqttPacket::PubAck { packet_id: id }),
        PacketType::PubRec => packet_id_header(input, |id| MqttPacket::PubRec { packet_id: id }),
        PacketType::PubRel => packet_id_header(input, |id| MqttPacket::PubRel { packet_id: id }),
        PacketType::PubComp => packet_id_header(input, |id| MqttPacket::PubComp { packet_id: id }),
        PacketType::Subscribe => subscribe_packet(input),
        PacketType::SubAck => sub_ack_packet(input),
        PacketType::Unsubscribe => unsubscribe_packet(input),
        PacketType::UnsubAck => {
            packet_id_header(input, |id| MqttPacket::UnsubAck { packet_id: id })
        }
        PacketType::PingReq => Ok((input, MqttPacket::PingReq)),
        PacketType::PingResp => Ok((input, MqttPacket::PingResp)),
        PacketType::Disconnect => Ok((input, MqttPacket::Disconnect)),
    }
}

fn packet_flags<'a>(
    ty: PacketType,
) -> impl Fn((&'a [u8], usize)) -> ParserResult<'a, (&'a [u8], usize), (PacketType, PacketFlags)> {
    move |input: (&'a [u8], usize)| {
        use self::PacketType::*;

        let (rest, received) = take_bits(4usize)(input)?;

        let expected: u8 = match ty {
            Publish => received,
            Subscribe | Unsubscribe => 0b0010,
            _ => 0b0000,
        };

        let res = if received == expected {
            Ok(received)
        } else {
            Err(Error::UnexpectedPacketFlags {
                expected,
                ty,
                received,
            })
        };

        res.and_then(|b| PacketFlags::try_from(b))
            .map(|f| (rest, (ty, f)))
            .map_err(|e| Err::Error(ParserError::some(e)))
    }
}

pub(crate) fn packet<'a>(input: &'a [u8]) -> ParserResult<&'a [u8], MqttPacket<'a>> {
    let (rest, (ty, flags)) = bits(flat_map(
        map_res_err(take_bits(4usize), |b: u8| {
            PacketType::try_from(b).map_err(|e| ParserError::some(e))
        }),
        packet_flags,
    ))(input)?;
    map_parser(
        flat_map(vle, take),
        add_error(complete(packet_body(ty, flags)), || Error::IncompletePacket),
    )(rest)
}
