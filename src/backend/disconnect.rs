use std::time::Duration;

use futures::sync::oneshot::{channel, Receiver};
use tokio_io::{AsyncWrite, AsyncRead};

use super::mqtt_loop::{Loop, LoopStatus, DisconnectState};
use persistence::Persistence;
use proto::MqttPacket;
use errors::{Error, ErrorKind, Result};

pub struct Disconnect {
    packet: MqttPacket,
    timeout: Option<u64>
}

impl ResponseType for Disconnect {
    type Item = Receiver<Result<bool>>;
    type Error = Error;
}

impl<I: 'static, P: 'static> Handler<Disconnect, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {
    fn handle(&mut self, msg: Disconnect, ctx: &mut FramedContext<Self>) -> Response<Self, Disconnect> {
        use self::LoopStatus::*;

        match self.status {
            Some(Connecting(_, _)) |
            Some(Disconnected) |
            Some(Disconnecting(_, _)) => {
                Loop::reply_error(Error::from(ErrorKind::ClientUnavailable))
            }
            Some(Connected) => {
                let (tx, rx) = channel::<Result<bool>>();
                if let Some(t) = msg.timeout {
                    let handle = ctx.notify(DisconnectTimeout, Duration::from_secs(t));
                    self.status = Some(Disconnecting(tx,
                                                     DisconnectState::Waiting(handle)));
                    Loop::reply(rx)
                } else {
                    self.status = Some(Disconnecting(tx, DisconnectState::Closing));
                    match ctx.send(msg.packet) {
                        Ok(()) => {
                            ctx.close();
                            Loop::reply(rx.into())
                        }
                        Err(_) => Loop::reply_error(Error::from(ErrorKind::LoopError))
                    }
                }
            }
            Some(PendingError(_)) => {
                let e = match self.status.take() {
                    Some(PendingError(e)) => e,
                    _ => unreachable!()
                };
                Loop::reply_error(e)
            }
            None => unreachable!()
        }
    }
}

pub struct DisconnectTimeout;

impl ResponseType for DisconnectTimeout {
    type Item = ();
    type Error = Error;
}

impl<I: 'static, P: 'static> Handler<DisconnectTimeout, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {
    // TODO: Implement behaviour for disconnect work timeout.
    fn handle(&mut self, msg: DisconnectTimeout, ctx: &mut FramedContext<Self>) -> Response<Self, DisconnectTimeout> {
        Loop::empty()
    }
}