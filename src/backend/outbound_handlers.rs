use bincode;
use actix::prelude::*;

use types::MqttFuture;
use persistence::Persistence;
use proto::{QualityOfService, PacketId, MqttPacket, MqttString};
use errors::{Result, ResultExt, ErrorKind, Error};
use backend::mqtt_loop::Loop;
use backend::{OneTimeKey, PublishState, ClientReturnSender};

trait OutboundHandler {
    fn handle<P>(&self, client: ClientReturnSender, data: &mut LoopData<P>) ->
        Result<Option<MqttPacket>> where P: Persistence;
}

pub struct Subscribe {
    subs: Vec<(String, QualityOfService)>
}

impl OutboundHandler for Subscribe {
    fn handle<P>(&self, client: ClientReturnSender, data: &mut LoopData<P>) ->
        Result<Option<MqttPacket>> where P: Persistence {

        let id = data.gen_packet_id();
        let mut new_subs = Vec::with_capacity(self.subs.len());
        for (topic, qos) in self.subs {
            new_subs.push((MqttString::from_str(&topic)?, qos));
        }
        let packet = MqttPacket::subscribe_packet(id, new_subs);
        data.one_time.insert(OneTimeKey::Subscribe(id), client);
        Ok(Some(packet))
    }
}

pub struct Unsubscribe {
    subs: Vec<String>
}

impl OutboundHandler for Unsubscribe {
    fn handle<P>(&self, client: ClientReturnSender, data: &mut LoopData<P>) ->
        Result<Option<MqttPacket>> where P: Persistence {
        let id = data.gen_packet_id();
        let mut new_subs = Vec::with_capacity(self.subs.len());
        for topic in self.subs {
            new_subs.push(MqttString::from_str(&topic)?);
        }
        let packet = MqttPacket::unsubscribe_packet(id, new_subs);
        data.one_time.insert(OneTimeKey::Unsubscribe(id), client);
        Ok(Some(packet))
    }
}

pub fn ping_req_handler<P>((packet, client): RequestTuple, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let res = if data.ping_subs.is_empty() {
        Some(packet)
    } else {
        None
    };
    data.ping_subs.push(client);
    Ok(res)
}

struct Publish {
    topic: String,
    qos: QualityOfService,
    retain: bool,
    msg: Vec<u8>
}



impl ResponseType for ClientPublish {
    type Item = MqttFuture<()>;
    type Error = Error;
}

impl<P: 'static> Handler<ClientPublish> for Loop<P> {
    fn handle(&mut self, msg: ClientPublish, ctx: &mut FramedContext<A>) -> Response<Self, ClientPublish> {

        match packet.flags.qos() {
            QualityOfService::QoS0 => {},
            QualityOfService::QoS1 | QualityOfService::QoS2 => {
                let id = packet.headers.get::<PacketId>().unwrap();

                let ser = packet.encode().unwrap();
                let key = match data.persistence.put(ser) {
                    Ok(k) => k,
                    Err(e) => return Err(e).chain_err(|| ErrorKind::PersistenceError)
                };
                data.client_publish_state.insert(*&id,
                                                 PublishState::Sent(key, Some(client)));
            }
        }
        let packet = MqttPacket::publish_packet(msg.qos.clone(), msg.retain.clone(),
                                                msg.topic.clone(), 4, msg.msg.clone());
        ctx.send(packet)
    }
}

pub fn publish_handler<P>((packet, client): RequestTuple, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    Ok(Some(packet))
}
