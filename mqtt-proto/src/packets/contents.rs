use super::prelude::*;
use bytes::BytesMut;

fn encode_vle<B: BufMut>(num: usize, out: &mut B) -> Result<()> {
    let mut val: usize = num;

    if num > 268_435_455 {
        return Err(Error::PacketTooBig(num));
    }

    loop {
        let mut enc_byte: u8 = (val % 128) as u8;
        val /= 128;
        if val > 0 {
            enc_byte |= 128;
        }
        out.put_u8(enc_byte);
        if val <= 0 {
            break;
        }
    }
    Ok(())
}

fn encode_headers_payload<'a, B, H, P>(out: &mut B, hdr: &'a H, pld: &'a P) -> Result<()>
    where B: BufMut, H: Headers<'a>, P: Payload<'a, H>
{
    encode_vle(hdr.encoded_length() + pld.encoded_length(), out)?;
    hdr.encode(out);
    pld.encode(out);
    Ok(())
}

fn encode_headers<'a, B: BufMut, H: Headers<'a>>(out: &mut B, hdr: &'a H) -> Result<()> {
    encode_vle(hdr.encoded_length(), out)?;
    hdr.encode(out);
    Ok(())
}

fn encode_empty<B: BufMut>(out: &mut B) -> Result<()> {
    encode_vle(0, out)?;
    Ok(())
}

fn length(payload_size: usize) -> Option<usize> {
    let mut buf = BytesMut::with_capacity(4);
    if let Ok(()) = encode_vle(payload_size, &mut buf) {
        Some((4 - buf.remaining_mut()) + payload_size)
    } else {
        None
    }
}

fn parse_headers_payload<'a, H, P, C>(input: &'a [u8], flags: &PacketFlags, build: C)
    -> IResult<&'a [u8], Contents<'a>>
    where H: Headers<'a>, P: Payload<'a, H>, C: Fn(H, P) -> Contents<'a>
{
    let (p_bytes, headers) = Headers::parse(input, flags)?;
    let (remain, payload) = complete!(p_bytes, call!(Payload::parse, flags, &headers))?;
    Ok((remain, build(headers,payload)))
}

fn parse_headers<'a, H, C>(input: &'a [u8], flags: &PacketFlags, build: C)
    -> IResult<&'a [u8], Contents<'a>>
    where H: Headers<'a>, C: Fn(H) -> Contents<'a>
{
    let (remain, headers) = complete!(input, call!(Headers::parse, flags))?;
    Ok((remain, build(headers)))
}

impl<'a> Contents<'a> {
    pub fn packet_type(&self) -> PacketType {
        use self::Contents::*;

        match self {
            Connect{headers: _, payload: _} => PacketType::Connect,
            ConnectAck(_) => PacketType::ConnectAck,
            Publish{headers: _, payload: _} => PacketType::Publish,
            PubAck(_) => PacketType::PubAck,
            PubRec(_) => PacketType::PubRec,
            PubRel(_) => PacketType::PubRel,
            PubComp(_) => PacketType::PubComp,
            Subscribe {headers: _, payload: _} => PacketType::Subscribe,
            SubAck {headers: _, payload: _} => PacketType::SubAck,
            Unsubscribe {headers: _, payload: _} => PacketType::Unsubscribe,
            UnsubAck(_) => PacketType::UnsubAck,
            PingReq => PacketType::PingReq,
            PingResp => PacketType::PingResp,
            Disconnect => PacketType::Disconnect
        }
    }

    pub fn encode<B: BufMut>(&self, out: &mut B) -> Result<()> {
        use self::Contents::*;

        match self {
            Connect{headers, payload} => encode_headers_payload(out, headers, payload),
            ConnectAck(h) => encode_headers(out, h),
            Publish{headers, payload} => encode_headers_payload(out, headers, payload),
            PubAck(h) => encode_headers(out, h),
            PubRec(h) => encode_headers(out, h),
            PubRel(h) => encode_headers(out, h),
            PubComp(h) => encode_headers(out, h),
            Subscribe {headers, payload} => encode_headers_payload(out, headers, payload),
            SubAck {headers, payload} => encode_headers_payload(out, headers, payload),
            Unsubscribe {headers, payload} => encode_headers_payload(out, headers, payload),
            UnsubAck(h) => encode_headers(out, h),
            PingReq | PingResp | Disconnect => encode_empty(out)
        }
    }

    pub fn len(&self) -> Option<usize> {
        use self::Contents::*;

        match self {
            Connect{headers, payload} => length(headers.encoded_length() + payload.encoded_length()),
            ConnectAck(h) => length(h.encoded_length()),
            Publish{headers, payload} => length(headers.encoded_length() + payload.encoded_length()),
            PubAck(h) => length(h.encoded_length()),
            PubRec(h) => length(h.encoded_length()),
            PubRel(h) => length(h.encoded_length()),
            PubComp(h) => length(h.encoded_length()),
            Subscribe {headers, payload} => length(headers.encoded_length() + payload.encoded_length()),
            SubAck {headers, payload} => length(headers.encoded_length() + payload.encoded_length()),
            Unsubscribe {headers, payload} => length(headers.encoded_length() + payload.encoded_length()),
            UnsubAck(h) => length(h.encoded_length()),
            PingReq | PingResp | Disconnect => Some(0)
        }
    }

    pub fn parse(input: &'a [u8], ty: &PacketType, flags: &PacketFlags) -> IResult<&'a [u8], Contents<'a>> {
        use super::PacketType::*;

        match ty {
            Connect => parse_headers_payload(input, flags, |headers, payload| Contents::Connect{
                headers,
                payload
            }),
            ConnectAck => parse_headers(input, flags, |headers| Contents::ConnectAck(headers)),
            Publish => parse_headers_payload(input, flags, |headers, payload| Contents::Publish{
                headers,
                payload
            }),
            PubAck => parse_headers(input, flags, |headers| Contents::PubAck(headers)),
            PubRec => parse_headers(input, flags, |headers| Contents::PubRec(headers)),
            PubRel => parse_headers(input, flags, |headers| Contents::PubRel(headers)),
            PubComp => parse_headers(input, flags, |headers| Contents::PubComp(headers)),
            Subscribe => parse_headers_payload(input, flags, |headers, payload| Contents::Subscribe{
                headers,
                payload
            }),
            SubAck => parse_headers_payload(input, flags, |headers, payload| Contents::SubAck{
                headers,
                payload
            }),
            Unsubscribe => parse_headers_payload(input, flags, |headers, payload| Contents::Subscribe{
                headers,
                payload
            }),
            UnsubAck => parse_headers(input, flags, |headers| Contents::UnsubAck(headers)),
            PingReq => Ok((input, Contents::PingReq)),
            PingResp => Ok((input, Contents::PingResp)),
            Disconnect => Ok((input, Contents::Disconnect))
        }
    }
}

