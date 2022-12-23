use std::collections::HashMap;
use std::pin::Pin;

use tokio::prelude::*;
use futures::task::{Context, Poll};
use mqtt3_proto::MqttPacket;

use crate::config::Config;
use crate::errors::*;
use crate::topic_filter::TopicFilter;
use crate::backend::{ReactorCommand, PacketFilter};
use crate::backend::channel::{LoopSender, loop_return, Subscription, SubscribeSink};
use crate::backend::codec::MqttPacketBuf;
use crate::backend::state::LoopState;

pub struct ReactorClient<'a> {
    sender: LoopSender<'a>
}

impl<'a> ReactorClient<'a> {
    pub fn new(sender: LoopSender<'a>) -> ReactorClient<'a> {
        ReactorClient {
            sender,
        }
    }

    pub fn connect(&'a mut self, io_read: &'a mut dyn AsyncRead, io_write: &'a mut dyn AsyncWrite, config: &'a Config<'a>) -> impl Future<Output = MqttResult<'a, Subscription<'a>>>{
        let (ret, recv) = loop_return();
        let command = ReactorCommand::Connect {
            io_read,
            io_write,
            config,
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }

    /// Request a packet be sent by the loop, returning the packet id given to it if required.
    /// The returned future will resolve when the packet is fully sent by the loop, or when an error occurs.
    pub fn send_packet(&'a mut self, packet: MqttPacket<'a>) -> impl Future<Output = MqttResult<'a, ()>> {
        let (ret, recv) = loop_return();
        let command = ReactorCommand::SendPacket {
            packet,
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }

    /// Await the arrival of a packet that matches the given filter.
    pub fn await_response<F: Into<PacketFilter>>(&'a mut self, filter: F) -> impl Future<Output=MqttResult<'a, MqttPacketBuf<'a>>> {
        let (ret, recv) = loop_return();
        let command = ReactorCommand::AwaitResponse {
            filter: filter.into(),
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }

    pub fn subscribe(&'a mut self, filter: &'a String) -> impl Future<Output = MqttResult<'a, Subscription<'a>>> {
        let (ret, recv) = loop_return();
        let command = ReactorCommand::Subscribe {
            filter,
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }

    pub fn unsubscribe(&'a mut self, filter: &'a String) -> impl Future<Output = MqttResult<'a, ()>> {
        let (ret, recv) = loop_return();
        let command = ReactorCommand::Unsubscribe {
            filter,
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }

    pub fn disconnect(&'a mut self, timeout: Option<u64>) -> impl Future<Output = MqttResult<'a, bool>> {
        let (ret, recv) = loop_return();
        let command = ReactorCommand::Disconnect {
            timeout,
            ret,
        };
        async {
            self.sender.send(command).await?;
            recv.await
        }
    }
}

pub struct SharedState<'a> {
    // Provides the next packet ID
    pub next_packet_id: usize,
    // Sink for app messages that come from old subs.
    pub lost_mailbox: Option<SubscribeSink<'a>>,
    // Current subscriptions.
    pub subscriptions: HashMap<String, (TopicFilter, SubscribeSink<'a>)>,
}

impl<'a> SharedState<'a> {
    pub fn new() -> SharedState<'a> {
        SharedState {
            next_packet_id: 0,
            lost_mailbox: None,
            subscriptions: HashMap::new(),
        }
    }

    pub fn gen_packet_id(&mut self) -> usize {
        let ret = self.next_packet_id;
        self.next_packet_id += 1;
        ret
    }
}

/// The event loop used by this library to complete the protocol flow of MQTT uses a proactor design
/// pattern to achieve concurrency between client requests and requests from the network.
///
/// All events come from two sources, the client interface or the server. Requests from either
/// source are packaged in a enum for the main loop to demux and route to asynchronous handlers.
pub struct Reactor<'a> {
    // Provided configuration for the client
    config: &'a Config<'a>,
    // Status of the loop
    state: Box<dyn LoopState<'a>>,
}

impl<'a> Future for Reactor<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        unimplemented!()
    }
}
