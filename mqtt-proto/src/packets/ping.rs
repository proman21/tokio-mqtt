use super::prelude::*;

pub fn ping_req<'a>() -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::PingReq
    }
}

pub fn ping_resp<'a>() -> MqttPacket<'a> {
    MqttPacket {
        flags: PacketFlags::empty(),
        contents: Contents::PingResp
    }
}
