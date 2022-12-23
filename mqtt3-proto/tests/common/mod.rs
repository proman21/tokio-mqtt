pub use mqtt3_proto::{MqttPacket, Error, MqttString};
pub use std::convert::TryInto;

pub fn check_encode(packet: MqttPacket, packet_data: &[u8]) {
    let mut buf: Vec<u8> = Vec::with_capacity(packet_data.len());
    assert_eq!(packet.encode(&mut buf), Ok(()));
    assert_eq!(buf, packet_data);
}