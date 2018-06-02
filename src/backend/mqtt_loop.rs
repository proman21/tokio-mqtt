use std::collections::HashMap;
use std::time::Duration;
use std::marker::PhantomData;

use actix::prelude::*;
use futures::sync::oneshot::{channel, Sender, Receiver};
use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver};
use tokio_io::{AsyncRead, AsyncWrite};

use errors::{Error, ErrorKind, Result};
use persistence::Persistence;
use proto::{MqttPacket, PacketType, PacketId};
use client::Config;
use super::ping::{PingRequest};
use super::codec::MqttCodec;
use super::{
    ClientReturnSender,
    ClientReturn,
    SubscriptionSender,
    PublishState,
    OneTimeKey,
    TopicFilter,
    LoopMessage,
    LoopMpscReceiver
};
use super::outbound_handlers::*;

pub enum LoopStatus {
    // Loop is disconnected from the server
    Disconnected,
    // Loop waits for either a conn_ack or the timeout
    Connecting {
        ret: ClientReturnSender<Option<Subscription>>
    },
    // Loop is connected and operates normally
    Connected,
    // Loop is in the process of disconnecting
    Disconnecting {
        ret: ClientReturnSender<bool>,
        state: DisconnectState
    },
    // Loop has an error that has not been reported to the client
    PendingError(Error),
}

pub enum DisconnectState {
    // Waiting for work to finish
    Waiting,
    // Closing the connection
    Closing
}

pub struct LoopData<P> where P: Persistence {
    // Provides the next packet ID
    next_packet_id: usize,
    // Sink for app messages that come from old subs. REUSABLE
    pub session_subs: Option<SubscriptionSender>,
    // Server publishing state tracker for QoS1/2 messages. REUSABLE
    pub server_publish_state: HashMap<PacketId, PublishState>,
    // Client publish state tracker for QoS1/2 messages. REUSABLE
    pub client_publish_state: HashMap<PacketId, PublishState>,
    // Map of unacknowledged topic subscriptions
    pub unack_subs: HashMap<PacketId, Vec<String>>,
    // Map of subscribers waiting for a ping response
    pub ping_subs: Vec<ClientReturnSender<()>>,
    // Current subscriptions. REUSABLE
    pub subscriptions: HashMap<String, (TopicFilter, SubscriptionSender)>,
    // Persistence cache. REUSABLE
    pub persistence: P
}

impl<P> LoopData<P> where P: Persistence {
    pub fn new(p: P) -> LoopData<P> {
        LoopData {
            next_packet_id: 0,
            session_subs: None,
            server_publish_state: HashMap::new(),
            client_publish_state: HashMap::new(),
            unack_subs: HashMap::new(),
            ping_subs: Vec::new(),
            subscriptions: HashMap::new(),
            persistence: p
        }
    }

    pub fn gen_packet_id(&mut self) -> usize {
        let ret = self.next_packet_id;
        self.next_packet_id += 1;
        ret
    }
}

/// The loop used by this library to complete the protocol flow of MQTT uses a reactor design
/// pattern to achieve concurrency between client requests and requests from the network.
///
/// All events come from two sources, the client interface or the server. Requests from either
/// source are packaged in a enum for the main loop to demux and route to synchronous handlers.
pub struct Loop<P> where P: Persistence {
    // Provided configuration for the client
    config: Config,
    // Status of the loop
    status: Option<LoopStatus>,
    // Loop data
    data: LoopData<P>,
    // Queue for incoming messages to the loop
    queue: LoopMpscReceiver<LoopMessage>,
    // Queue to send packets out
    out: UnboundedSender<MqttPacket>,
    // Queue to request timeouts
    timeout: UnboundedSender<TimeoutRequest>
}

impl<P: 'static> Loop<P> where P: Persistence {
    pub fn local(persist: P, config: Config, q: LoopMpscReceiver<LoopMessage>, handle: &Handle) -> Loop<P> {
        Loop {
            config,
            status: Some(Disconnected),
            data: LoopData::new(persist),
            queue: q,
            timer: None
        }
    }

    fn remote(persist: P, config: Config, q: LoopMpscReceiver<LoopMessage>, handle: &Handle) -> Loop<P>{

    }
}

impl<P: 'static> Handler<ClientRequest, Error> for Loop<P> where P: Persistence {
    fn handle(&mut self, msg: ClientRequest, ctx: &mut Context<Self>) -> Response<Self, ClientRequest> {
        use self::LoopStatus::*;

        match self.status {
            Some(Connecting(_, _)) |
            Some(Disconnected) |
            Some(Disconnecting(_, _)) => {
                Loop::reply_error(Error::from(ErrorKind::ClientUnavailable))
            },
            Some(Connected) => {
                let (tx, rx) = channel::<Result<ClientReturn>>();
                let res = match msg.packet.ty {
                    PacketType::Subscribe => subscribe_handler((msg.packet, tx), &mut self.data),
                    PacketType::Unsubscribe => unsubscribe_handler((msg.packet, tx), &mut self.data),
                    PacketType::PingReq => ping_req_handler((msg.packet, tx), &mut self.data),
                    PacketType::Publish => publish_handler((msg.packet, tx), &mut self.data),
                    _ => unreachable!()
                };
                let send = res.and_then(|p: Option<MqttPacket>| {
                    if let Some(packet) = p {
                        ctx.send(packet)
                            .map(|_| true)
                            .map_err(|e| Error::from(ErrorKind::LoopError))
                    } else {
                        Ok(false)
                    }
                });

                match send {
                    Ok(sent) => {
                        if sent {
                            self.timer = self.timer.map(|h| {
                                ctx.cancel_future(h);
                                ctx.notify(PingRequest, self.keep_alive_dur())
                            });
                        }
                        Loop::reply(rx)
                    },
                    Err(e) => Loop::reply_error(e)
                }
            },
            Some(PendingError(_)) => {
                let e = match self.status.take() {
                    Some(PendingError(e)) => e,
                    _ => unreachable!()
                };
                self.status = Some(Disconnected);
                Loop::reply_error(e)
            },
            None => unreachable!()
        }
    }
}
