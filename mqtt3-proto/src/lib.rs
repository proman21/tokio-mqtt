#[macro_use]
extern crate snafu;
#[macro_use]
extern crate derive_builder;
#[macro_use]
extern crate bitflags;
extern crate nom;
#[macro_use]
extern crate enum_primitive;
extern crate bytes;

mod errors;
mod packets;
mod parsers;
pub mod types;

pub use types::*;
pub use errors::*;

/// A enumeration of possible control packets used in the MQTT Protocol.
///
/// This type abstracts the on-wire representation of an MQTT Control packet to prevent the formation of invalid
/// packets. It also attempts to be a memory efficient as possible using a zero-copy parser.
#[derive(Clone, Debug, PartialEq)]
pub enum MqttPacket<'a> {
    Connect {
        protocol_level: ProtoLvl,
        clean_session: bool,
        keep_alive: u16,
        client_id: MqttString<'a>,
        lwt: Option<LWTMessage<'a, &'a [u8]>>,
        credentials: Option<Credentials<'a, &'a [u8]>>,
    },
    ConnAck {
        result: Result<ConnAckFlags, ConnectError>,
    },
    Publish {
        pub_type: PublishType,
        retain: bool,
        topic_name: MqttString<'a>,
        message: &'a [u8],
    },
    PubAck {
        packet_id: u16,
    },
    PubRec {
        packet_id: u16,
    },
    PubRel {
        packet_id: u16,
    },
    PubComp {
        packet_id: u16,
    },
    Subscribe {
        packet_id: u16,
        subscriptions: Vec<SubscriptionTuple<'a>>,
    },
    SubAck {
        packet_id: u16,
        results: Vec<SubAckReturnCode>,
    },
    Unsubscribe {
        packet_id: u16,
        topics: Vec<MqttString<'a>>,
    },
    UnsubAck {
        packet_id: u16,
    },
    PingReq,
    PingResp,
    Disconnect,
}
