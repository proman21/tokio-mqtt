extern crate mqtt3_proto;

mod common;

use common::*;
use mqtt3_proto::PublishType;

#[test]
fn qos0() {
    let packet_data = vec![
        0b00110000, // Packet Type and Flags
        7, // Remaining Length
        0, 3, 0x61, 0x2F, 0x62, // Topic Name
        0, 0, // Payload
    ];

    let packet = MqttPacket::Publish {
        pub_type: PublishType::QoS0,
        retain: false,
        topic_name: "a/b".try_into().unwrap(),
        message: &[][..]
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn qos1() {
    let packet_data = vec![
        0b00110010, // Packet Type and Flags
        23, // Remaining Length
        0, 3, 0x61, 0x2F, 0x62, // Topic Name
        0, 20, // Packet ID,
        0, 14, 0x83, 0xd6, 0xa6, 0xb3, 0x75, 0x4d, 0xb8, 0x13, 0xb2, 0x3b, 0x90, 0xf1, 0x13, 0x69, // Payload
    ];
    let message_data = [0x83, 0xd6, 0xa6, 0xb3, 0x75, 0x4d, 0xb8, 0x13, 0xb2, 0x3b, 0x90, 0xf1, 0x13, 0x69,];

    let packet = MqttPacket::Publish {
        pub_type: PublishType::QoS1{ packet_id: 20, dup: false },
        retain: false,
        topic_name: "a/b".try_into().unwrap(),
        message: &message_data[..]
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn qos2() {
    let packet_data = vec![
        0b00111101, // Packet Type and Flags
        23, // Remaining Length
        0, 3, 0x61, 0x2F, 0x62, // Topic Name
        0, 20, // Packet ID,
        0, 14, 0x83, 0xd6, 0xa6, 0xb3, 0x75, 0x4d, 0xb8, 0x13, 0xb2, 0x3b, 0x90, 0xf1, 0x13, 0x69, // Payload
    ];
    let message_data = [0x83, 0xd6, 0xa6, 0xb3, 0x75, 0x4d, 0xb8, 0x13, 0xb2, 0x3b, 0x90, 0xf1, 0x13, 0x69,];

    let packet = MqttPacket::Publish {
        pub_type: PublishType::QoS2{ packet_id: 20, dup: true },
        retain: true,
        topic_name: "a/b".try_into().unwrap(),
        message: &message_data[..]
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    check_encode(packet, &packet_data);
}

#[test]
fn invalid_flags() {
    let packet_data = vec![
        0b00111110, // Packet Type and Flags
        23, // Remaining Length
        0, 3, 0x61, 0x2F, 0x62, // Topic Name
        0, 20, // Packet ID,
        0, 14, 0x83, 0xd6, 0xa6, 0xb3, 0x75, 0x4d, 0xb8, 0x13, 0xb2, 0x3b, 0x90, 0xf1, 0x13, 0x69, // Payload
    ];

    assert_eq!(MqttPacket::from_buf(&packet_data), Err(Error::InvalidPacketFlag{ flags: 0b1110 }));
}