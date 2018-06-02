use std::collections::hash_map::Entry;
use std::ops::Deref;

use bincode;
use bytes::Bytes;
use futures::Stream;
use futures::sync::mpsc::unbounded;

use proto::{MqttPacket, Payload, PacketId, QualityOfService, TopicName, TopicFilter};
use persistence::Persistence;
use errors::{Result, Error, ErrorKind, ResultExt};
use errors::proto::{ErrorKind as ProtoErrorKind};
use backend::mqtt_loop::LoopData;
use backend::{OneTimeKey, ClientReturn, SubItem, PublishState};
use types::SubscriptionStream;

pub fn sub_ack_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let packet_id = packet.headers.get::<PacketId>().unwrap();
    let (o, c) = match data.one_time.entry(OneTimeKey::Subscribe(*packet_id)) {
        Entry::Vacant(v) => {
            return Err(ErrorKind::from(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone())).into());
        },
        Entry::Occupied(o) => o.remove()
    };
    // Validate each return code
    let orig = match o.payload {
        Payload::Subscribe(v) => v,
        _ => unreachable!()
    };
    let ret_codes = match packet.payload {
        Payload::SubAck(v) => v,
        _ => unreachable!()
    };
    let mut collect: Vec<Result<(SubscriptionStream, QualityOfService)>> = Vec::new();
    for (sub, ret) in orig.iter().zip(ret_codes.iter()) {
        if ret.is_ok() {
            let (tx, rx) = unbounded::<Result<SubItem>>();
            let filter = TopicFilter::from_string(&sub.topic)?;
            data.subscriptions.insert(sub.topic.clone().into(), (filter, tx));
            let res: Result<(SubscriptionStream, QualityOfService)> = Ok((rx.into(), sub.qos));
            collect.push(res);
        } else {
            collect.push(Err(ErrorKind::from(
                ProtoErrorKind::SubscriptionRejected(
                    sub.topic.clone().into(), sub.qos)
                ).into()
            ));
        }
    }
    let _ = c.send(Ok(ClientReturn::Ongoing(collect)));
    Ok(None)
}

pub fn unsub_ack_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let pid = packet.headers.get::<PacketId>().unwrap();
    let (o, c) = match data.one_time.entry(OneTimeKey::Unsubscribe(*pid)) {
        Entry::Vacant(v) => {
            return Err(ErrorKind::from(ProtoErrorKind::UnexpectedResponse(
                packet.ty.clone())).into());
        },
        Entry::Occupied(o) => o.remove()
    };
    let topics = match o.payload {
        Payload::Unsubscribe(v) => v,
        _ => unreachable!()
    };
    // Remove subscriptions from loop
    for topic in topics {
        let _ = data.subscriptions.remove(topic.deref());
    }
    let _ = c.send(Ok(ClientReturn::Onetime(None)));
    Ok(None)
}

pub fn ping_resp_handler<P>(data: &mut LoopData<P>) -> Result<Option<MqttPacket>>
    where P: Persistence {

    for client in data.ping_subs.drain(0..) {
        let _ = client.send(Ok(ClientReturn::Onetime(None)));
    }

    Ok(None)
}

pub fn publish_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {
    match packet.flags.qos() {
        QualityOfService::QoS0 => {
            let topic = packet.headers.get::<TopicName>().unwrap();
            let payload = match packet.payload {
                Payload::Application(d) => d,
                _ => unreachable!()
            };
            for &(ref filter, ref sender) in data.subscriptions.values() {
                if filter.match_topic(&topic) {
                    let _ = sender.unbounded_send(Ok(SubItem(topic.clone(), payload.clone())));
                }
            }
            Ok(None)
        },
        QualityOfService::QoS1 => {
            let id = packet.headers.get::<PacketId>().unwrap();

            let topic = packet.headers.get::<TopicName>().unwrap();
            let payload = match packet.payload {
                Payload::Application(d) => d,
                _ => unreachable!()
            };
            for &(ref filter, ref sender) in data.subscriptions.values() {
                if filter.match_topic(&topic) {
                    let _ = sender.unbounded_send(Ok(SubItem(topic.clone(), payload.clone())));
                }
            }

            // Send back an acknowledgement
            Ok(Some(MqttPacket::pub_ack_packet(*id)))
        },
        QualityOfService::QoS2 => {
            let id = packet.headers.get::<PacketId>().unwrap();

            // Check if we have an existing publish with the same id
            if data.server_publish_state.contains_key(&id) {
                return Err(ProtoErrorKind::QualityOfServiceError(
                    packet.flags.qos(),
                    format!("Duplicate publish recieved with same Packet ID: {}", *id)
                ).into())
            } else {
                data.server_publish_state.insert(*&id, PublishState::Received(packet));
                // Send PUBREC
                Ok(Some(MqttPacket::pub_rec_packet(*id)))
            }
        }
    }
}

pub fn pub_ack_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let id = packet.headers.get::<PacketId>().unwrap();
    if data.client_publish_state.contains_key(&id) {
        let (p_key, sender) = match data.client_publish_state.remove(&id) {
            Some(PublishState::Sent(p, s)) => (p, s),
            _ => unreachable!()
        };
        match data.persistence.remove(&p_key) {
            Ok(_) => {
                if let Some(s) = sender {
                    let _ = s.send(Ok(ClientReturn::Onetime(None)));
                }
            },
            Err(e) => {
                if let Some(s) = sender {
                    let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                }
                return Err(ErrorKind::PersistenceError.into())
            }
        }
    } else {
        return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
    }

    Ok(None)
}

pub fn pub_rec_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {
    let id = packet.headers.get::<PacketId>().unwrap();
    if data.client_publish_state.contains_key(&id) {
        let (p_key, sender) = match data.client_publish_state.remove(&id) {
            Some(PublishState::Sent(p, s)) => (p, s),
            _ => unreachable!()
        };
        if let Err(e) = data.persistence.remove(&p_key) {
            return Err(e).chain_err(|| ErrorKind::PersistenceError)
        }
        let rel = MqttPacket::pub_rel_packet(*id);
        let ser = bincode::serialize(&rel, bincode::Infinite).unwrap();
        let key = match data.persistence.append(ser) {
            Ok(k) => k,
            Err(e) => {
                if let Some(s) = sender {
                    let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                }
                return Err(ErrorKind::PersistenceError.into())
            }
        };
        let _ = data.client_publish_state.insert(*&id, PublishState::Released(key, sender));

        Ok(Some(rel))
    } else {
        return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
    }
}

pub fn pub_rel_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let id = packet.headers.get::<PacketId>().unwrap();
    if data.server_publish_state.contains_key(&id) {
        let orig = match data.server_publish_state.remove(&id) {
            Some(PublishState::Received(o)) => o,
            _ => unreachable!()
        };

        let topic = orig.headers.get::<TopicName>().unwrap();
        let payload = match orig.payload {
            Payload::Application(d) => d,
            _ => unreachable!()
        };
        for &(ref filter, ref sender) in data.subscriptions.values() {
            if filter.match_topic(&topic) {
                let _ = sender.unbounded_send(Ok(SubItem(topic.clone(), payload.clone())));
            }
        }

        Ok(Some(MqttPacket::pub_comp_packet(*id)))
    } else {
        return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
    }
}

pub fn pub_comp_handler<P>(packet: MqttPacket, data: &mut LoopData<P>) ->
    Result<Option<MqttPacket>> where P: Persistence {

    let id = packet.headers.get::<PacketId>().unwrap();
    if data.client_publish_state.contains_key(&id) {
        let (p_key, sender) = match data.client_publish_state.remove(&id) {
            Some(PublishState::Released(p, s)) => (p, s),
            _ => unreachable!()
        };
        match data.persistence.remove(p_key) {
            Ok(_) => {
                if let Some(s) = sender {
                    let _ = s.send(Ok(ClientReturn::Onetime(None)));
                }
                Ok(None)
            },
            Err(e) => {
                if let Some(s) = sender {
                    let _ = s.send(Err(e).chain_err(|| ErrorKind::PersistenceError));
                }
                return Err(ErrorKind::PersistenceError.into())
            }
        }
    } else {
        return Err(ProtoErrorKind::UnexpectedResponse(packet.ty.clone()).into())
    }
}
