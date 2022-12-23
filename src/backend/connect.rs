use std::time::Duration;

use futures::sync::oneshot::channel;

use super::prelude::*;
use super::ClientReturn;
use crate::types::MqttFuture;
use mqtt3_proto::PacketType;

pub struct Connect {
    packet: MqttPacket,
    timeout: Option<u64>
}

impl Connect {
    pub fn new(packet: MqttPacket, timeout: Option<u64>) -> Connect {
        Connect {
            packet,
            timeout
        }
    }
}

impl ResponseType for Connect {
    type Item = MqttFuture<ClientReturn>;
    type Error = Error;
}

impl<P: 'static> Handler<Connect> for Loop<P> where P: Persistence {
    fn handle(&mut self, msg: Connect, ctx: &mut FramedContext<Self>) -> Response<Self, Connect> {
        use self::LoopStatus::*;

        match self.status {
            Some(Connecting(_, _)) |
            Some(Disconnected) |
            Some(Disconnecting(_, _)) => {
                Loop::reply_error(Error::from(ErrorKind::ClientUnavailable))
            },
            Some(Connected) => {
                Loop::reply_error(Error::from(ErrorKind::AlreadyConnected))
            },
            Some(PendingError(_)) => {
                let e = match self.status.take() {
                    Some(PendingError(e)) => e,
                    _ => unreachable!()
                };
                Loop::reply_error(e)
            }
            None => {
                let (tx, rx) = channel::<Result<ClientReturn>>();

                // Decide if we need to start a timeout
                let timer = msg.timeout.map(|t| ctx.notify(ConnectTimeout, Duration::from_secs(t)));

                match ctx.send(msg.packet) {
                    Ok(()) => {
                        self.status = Some(Connecting(timer, tx));
                        Loop::reply(rx.into())
                    },
                    Err(_) => Loop::reply_error(Error::from(ErrorKind::LoopError))
                }
            }
        }
    }
}

struct ConnectTimeout;

impl ResponseType for ConnectTimeout {
    type Item = ();
    type Error = Error;
}

impl<I: 'static, P: 'static> Handler<ConnectTimeout, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {

    fn handle(&mut self, msg: ConnectTimeout, ctx: &mut Self::Context)
        -> Response<Self, ConnectTimeout> {

        use self::LoopStatus::*;

        match self.status {
            Some(Connecting(_, _)) => {
                let client = match self.status.take() {
                    Some(Connecting(_, c)) => c,
                    _ => unreachable!()
                };

                let _ = client.send(Err(Error::from(ProtoErrorKind::ResponseTimeout(PacketType::Connect))));
                Loop::empty()
            },
            Some(PendingError(_)) => {
                let e = match self.status.take() {
                    Some(PendingError(e)) => e,
                    _ => unreachable!()
                };
                Loop::reply_error(e)
            }
            _ => Loop::empty()
        }
    }
}