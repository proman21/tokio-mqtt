use std::collections::HashMap;
use std::time::{Duration};
use std::pin::Pin;

use tokio::prelude::*;
use futures::task::{Context, Poll};
use futures::sink::Send;
use mqtt3_proto::MqttPacket;
use tokio::codec::{FramedRead, FramedWrite};
use tokio::timer::{delay_for, Delay};

use crate::config::Config;
use crate::backend::{ReactorCommand, PacketFilter};
use crate::backend::channel::{LoopReceiver, LoopRetSender};
use crate::errors::*;
use crate::backend::codec::{MqttCodec, MqttPacketBuf};

type MqttFramedRead<'a> = FramedRead<Box<dyn AsyncRead>, MqttCodec<'a>>;
type MqttFramedWrite<'a> = FramedWrite<Box<dyn AsyncWrite>, MqttCodec<'a>>;

struct WriteFuture<'a, S>
where S: Sink<MqttPacket<'a>, Error=Error<'a>> + std::marker::Unpin
{
    send: Send<'a, S, MqttPacket<'a>>,
    ret: LoopRetSender<'a, ()>,
}

impl<'a, S> WriteFuture<'a, S>
where S: Sink<MqttPacket<'a>, Error=Error<'a>> + std::marker::Unpin
{
    fn new(send: Send<'a, S, MqttPacket<'a>>, ret: LoopRetSender<'a, ()>) -> WriteFuture<'a, S> {
        WriteFuture {
            send,
            ret
        }
    }
}

impl<'a, S> Future for WriteFuture<'a, S>
where S: Sink<MqttPacket<'a>, Error=Error<'a>> + std::marker::Unpin
{
    type Output = MqttResult<'a, ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let res = ready!(self.send.poll(cx));
        Poll::Ready(self.ret.send(res))
    }
}

pub trait LoopState<'a> {
    fn poll_state(self: Box<Self>, cx: &mut Context<'a>) -> MqttResult<'a, Box<dyn LoopState<'a>>>;
}

pub struct Disconnected<'a> {
    client: LoopReceiver<'a>,
    router: HashMap<PacketFilter, LoopRetSender<'a, MqttPacketBuf<'a>>>,
}

impl<'a> LoopState<'a> for Disconnected<'a> {
    fn poll_state(self: Box<Self>, cx: &mut Context) -> MqttResult<'a, Box<dyn LoopState<'a>>> {
        while let Poll::Ready(Some(msg)) = self.client.poll_next(cx) {
            match msg {
                ReactorCommand::SendPacket{ packet, ret, .. } => {},
                ReactorCommand::AwaitResponse{ filter, ret } => {
                    self.router.insert(filter, ret);
                },
                ReactorCommand::Connect{ io_read, io_write, config } => {
                    let timer = if config.keep_alive != 0 {
                        Some(delay_for(Duration::from_secs(config.keep_alive.into())))
                    } else {
                        None
                    };


                    let new_state = Box::new(Connected {
                        client: self.client,
                        router: self.router,
                        reader: FramedRead::new(io_read, MqttCodec::new()),
                        writer,
                        config: config,
                        timer
                    });

                    return Ok(new_state)
                }
            }
        }

        Ok(self)
    }
}

pub struct Connected<'a> {
    client: LoopReceiver<'a>,
    router: HashMap<PacketFilter, LoopRetSender<'a, MqttPacketBuf<'a>>>,
    reader: MqttFramedRead<'a>,
    writer: MqttFramedWrite<'a>,
    config: &'a Config<'a>,
    timer: Option<Delay>,
}

impl<'a> LoopState<'a> for Connected<'a> {
    fn poll_state(self: Box<Self>, cx: &mut Context) -> MqttResult<'a, Box<dyn LoopState<'a>>> {
        unimplemented!()
    }
}