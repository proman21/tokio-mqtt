extern crate mqtt3_proto;

mod common;

use common::*;
use mqtt3_proto::{ConnAckFlags, ConnectError};

#[test]
fn basic_packet() {
    let packet_data = vec![
        0b00100000u8, // Packet Type and Flags
        2, // Remaining Length
        0b00000001, // Connect Ack Flags
        0, //Connect Return Code
    ];

    let packet = MqttPacket::ConnAck {
        result: Ok(ConnAckFlags::SP)
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn error() {
    let packet_data = vec![
        0b00100000u8, // Packet Type and Flags
        2, // Remaining Length
        0b00000000, // Connect Ack Flags
        2, //Connect Return Code
    ];

    let packet = MqttPacket::ConnAck {
        result: Err(ConnectError::ClientIdRejected)
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}