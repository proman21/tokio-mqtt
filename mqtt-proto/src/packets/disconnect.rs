use super::prelude::*;

pub fn disconnect<'a>() -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::Disconnect
    }
}
