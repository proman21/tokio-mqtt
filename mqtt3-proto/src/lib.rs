extern crate failure;
extern crate failure_derive;
#[macro_use] extern crate derive_builder;
#[macro_use] extern crate bitflags;
#[macro_use] extern crate nom;
#[macro_use] extern crate enum_primitive;
#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate bytes;

pub mod types;
mod parsers;
pub mod topic_filter;
pub mod errors;
mod packets;

pub use types::*;
pub use topic_filter::*;

/// A enumeration of possible control packets used in the MQTT Protocol.
///
/// This type abstracts the on-wire representation of an MQTT Control packet to prevent the formation of invalid
/// packets. It also attempts to be a memory efficient as possible using a zero-copy parser.
pub enum MqttPacket<'a> {
    Connect {
        protocol_level: ProtoLvl,
        clean_session: bool,
        keep_alive: u16,
        client_id: &'a str,
        lwt: Option<LWTMessage<&'a str, &'a [u8]>>,
        credentials: Option<Credentials<&'a str, &'a [u8]>>
    },
    ConnAck {
        session_present: bool,
        connect_return_code: ConnRetCode,
    },
    Publish {
        dup: bool,
        qos: QualityOfService,
        retain: bool,
        topic_name: &'a str,
        packet_id: Option<u16>,
        message: &'a [u8]
    },
    PubAck {
        packet_id: u16
    },
    PubRec{
        packet_id: u16
    },
    PubRel{
        packet_id: u16
    },
    PubComp{
        packet_id: u16
    },
    Subscribe {
        packet_id: u16,
        subscriptions: Vec<SubscriptionTuple<'a>>
    },
    SubAck {
        packet_id: u16,
        results: Vec<Option<QualityOfService>>
    },
    Unsubscribe {
        packet_id: u16,
        topics: Vec<&'a str>
    },
    UnsubAck {
        packet_id: u16
    },
    PingReq,
    PingResp,
    Disconnect
}
