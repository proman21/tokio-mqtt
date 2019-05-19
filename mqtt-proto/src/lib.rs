extern crate failure;
extern crate failure_derive;
#[macro_use] extern crate derive_builder;
#[macro_use] extern crate bitflags;
#[macro_use] extern crate nom;
#[macro_use] extern crate enum_primitive;
#[macro_use] extern crate lazy_static;
extern crate regex;
extern crate bytes;

#[cfg(test)]
#[macro_use] extern crate nom_test_helpers;

pub mod types;
pub mod parsers;
pub mod topic_filter;
pub mod errors;
pub mod packets;

pub use types::*;
pub use topic_filter::*;

pub use packets::Contents;

pub struct MqttPacket<'a> {
    flags: PacketFlags,
    contents: Contents<'a>
}
