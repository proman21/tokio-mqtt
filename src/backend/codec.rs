use std::marker::PhantomData;
use std::ops::Deref;

use tokio::codec::{Encoder, Decoder};
use bytes::{Bytes, BytesMut, BufMut};
use mqtt3_proto::MqttPacket;
use snafu::ResultExt;

use crate::errors::*;

pub struct MqttPacketBuf<'a> {
    buf: Bytes,
    packet: MqttPacket<'a>,
}

impl<'a> Deref for MqttPacketBuf<'a> {
    type Target = MqttPacket<'a>;

    fn deref(&self) -> &MqttPacket<'a> {
        &self.packet
    }
}

pub struct MqttCodec<'a>(PhantomData<&'a ()>);

impl<'a> MqttCodec<'a> {
    pub fn new() -> MqttCodec<'a> {
        MqttCodec(PhantomData)
    }
}

impl<'a> Encoder for MqttCodec<'a> {
    type Item = MqttPacket<'a>;
    type Error = Error<'a>;

    fn encode(&mut self, item: MqttPacket<'a>, dst: &mut BytesMut) -> Result<(), Error<'a>> {
        let len = item.len().context(ProtocolError)?;
        dst.reserve(len);
        item.encode(dst).context(ProtocolError)
    }
}

impl<'a> Decoder for MqttCodec<'a> {
    type Item = MqttPacketBuf<'a>;
    type Error = Error<'a>;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<MqttPacketBuf<'a>>, Error<'a>> {
        let buf = src.take().freeze();
        if let Some((rest, packet)) = MqttPacket::from_buf(&buf).context(ProtocolError)? {
            let remain = buf.slice_ref(rest);
            src.put(remain);
            Ok(Some(MqttPacketBuf {
                buf,
                packet
            }))
        } else {
            src.put(buf);
            Ok(None)
        }

    }
}
