use ::tokio_io::codec::{Encoder, Decoder};
use ::bytes::{BytesMut, BufMut};
use ::proto::MqttPacket;
use ::errors::{Error, ErrorKind};

pub struct MqttCodec;

impl Encoder for MqttCodec {
    type Item = MqttPacket;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if let Some(b) = item.encode() {
            dst.extend(b);
            Ok(())
        } else {
            bail!(ErrorKind::Msg("Encoding Error".into()))
        }
    }
}

impl Decoder for MqttCodec {
    type Item = MqttPacket;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let by = src.take();
        if let Some((packet, rest)) = MqttPacket::from_slice(&by)? {
            src.put(rest);
            Ok(Some(packet))
        } else {
            src.put(by);
            Ok(None)
        }

    }
}
