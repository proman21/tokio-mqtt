extern crate mqtt3_proto;

mod common;

use common::*;

#[test]
fn pub_ack() {
    let packet_data = vec![
        0b01000000u8, // Packet Type and Flags
        2, // Remaining Length
        0, 20, // Packet ID
    ];

    let packet = MqttPacket::PubAck {
        packet_id: 20
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn pub_rec() {
    let packet_data = vec![
        0b01010000u8, // Packet Type and Flags
        2, // Remaining Length
        0, 20, // Packet ID
    ];

    let packet = MqttPacket::PubRec {
        packet_id: 20
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn pub_rel() {
    let packet_data = vec![
        0b01100000u8, // Packet Type and Flags
        2, // Remaining Length
        0, 20, // Packet ID
    ];

    let packet = MqttPacket::PubRel {
        packet_id: 20
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn pub_comp() {
    let packet_data = vec![
        0b01110000u8, // Packet Type and Flags
        2, // Remaining Length
        0, 20, // Packet ID
    ];

    let packet = MqttPacket::PubComp {
        packet_id: 20
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}