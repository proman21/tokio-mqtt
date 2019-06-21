extern crate mqtt3_proto;

mod common;

use common::*;
use mqtt3_proto::{ProtoLvl, LWTMessage, QualityOfService, PacketType, Credentials};

#[test]
fn basic_packet() {
    let packet_data = vec![
        0b00010000u8, // Packet Type and Flags
        0x10, // Remaining Length
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Protocol Name
        4, // Protocol Level
        0b00000000, // Connect Flags
        0, 0, // Keep Alive
        0, 4, 0x4d, 0x51, 0x54, 0x54 // Client ID
    ];

    let packet = MqttPacket::Connect {
        protocol_level: ProtoLvl::V3_1_1,
        clean_session: false,
        keep_alive: 0,
        client_id: "MQTT".try_into().unwrap(),
        lwt: None,
        credentials: None
    };
    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    let partial = &packet_data[..9];
    assert_eq!(MqttPacket::from_buf(&partial), Ok(None));

    check_encode(packet, &packet_data);
}

#[test]
fn bad_packet_flags() {
    let packet_data = vec![
        0b00010101u8
    ];
    assert_eq!(MqttPacket::from_buf(&packet_data), Err(Error::UnexpectedPacketFlags {
        expected: 0b0000,
        ty: PacketType::Connect,
        received: 0b0101
    }))
}

#[test]
fn wrong_protocol_name() {
    let packet_data = vec![
        0b00010000u8, // Packet Type and Flags
        0x11, // Remaining Length
        0, 5, 0x4d, 0x51, 0x54, 0x54, 0x43,  // Protocol Name
        4, // Protocol Level
        0b00000000, // Connect Flags
        0, 0, // Keep Alive
        0, 4, 0x4d, 0x51, 0x54, 0x54 // Client ID
    ];

    assert_eq!(MqttPacket::from_buf(&packet_data), Err(Error::InvalidProtocolName{ name: "MQTTC".try_into().unwrap() }));
}

#[test]
fn with_lwt() {
    let packet_data = vec![
        0b00010000u8, // Packet Type and Flags
        0x23, // Remaining Length
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Protocol Name
        4, // Protocol Level
        0b00000100, // Connect Flags
        0, 30, // Keep Alive
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Client ID
        0, 0xE, 0x61, 0x2F, 0x74, 0x6F, 0x70, 0x69, 0x63, 0x2F, 0x73, 0x74, 0x72, 0x69, 0x6E, 0x67, // Will Topic
        0, 1, 1, // Will Message
    ];
    let packet = MqttPacket::Connect {
        protocol_level: ProtoLvl::V3_1_1,
        clean_session: false,
        keep_alive: 30,
        client_id: "MQTT".try_into().unwrap(),
        lwt: Some(LWTMessage {
            topic: "a/topic/string".try_into().unwrap(),
            qos: QualityOfService::QoS0,
            retain: false,
            message: &[1u8][..]
        }),
        credentials: None
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));
    
    check_encode(packet, &packet_data);
}

#[test]
fn with_credentials() {
    let packet_data = vec![
        0b00010000u8, // Packet Type and Flags
        0x24, // Remaining Length
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Protocol Name
        4, // Protocol Level
        0b11000000, // Connect Flags
        0, 0, // Keep Alive
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Client ID
        0, 0x8, 117, 115, 101, 114, 110, 97, 109, 101, // Username
        0, 0x8, 112, 97, 115, 115, 119, 111, 114, 100, // Password
    ];
    let packet = MqttPacket::Connect {
        protocol_level: ProtoLvl::V3_1_1,
        clean_session: false,
        keep_alive: 0,
        client_id: "MQTT".try_into().unwrap(),
        lwt: None,
        credentials: Some(Credentials{
            username: "username".try_into().unwrap(),
            password: Some(b"password")
        })
    };

    assert_eq!(MqttPacket::from_buf(&packet_data), Ok(Some((&[][..], packet.clone()))));

    check_encode(packet, &packet_data);
}

#[test]
fn missing_username() {
    let packet_data = vec![
        0b00010000u8, // Packet Type and Flags
        0x24, // Remaining Length
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Protocol Name
        4, // Protocol Level
        0b01000000, // Connect Flags
        0, 0, // Keep Alive
        0, 4, 0x4d, 0x51, 0x54, 0x54, // Client ID
        0, 0x8, 117, 115, 101, 114, 110, 97, 109, 101, // Username
        0, 0x8, 112, 97, 115, 115, 119, 111, 114, 100, // Password
    ];

    assert_eq!(MqttPacket::from_buf(&packet_data), Err(Error::InvalidConnectFlags{ flags: 0b01000000}));
}