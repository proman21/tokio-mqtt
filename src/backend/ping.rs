use std::time::Duration;

use futures::Future;
use futures::sync::oneshot::{channel, Canceled};
use actix::prelude::*;
use actix::{FramedContext, Handler, ResponseType};
use tokio_io::{AsyncRead, AsyncWrite};

use persistence::Persistence;
use proto::{MqttPacket, PacketType};
use backend::{Loop, ClientReturn};
use backend::mqtt_loop::{LoopStatus, DisconnectState};
use errors::{Error, ErrorKind, Result};
use errors::proto::ErrorKind as ProtoErrorKind;
use backend::outbound_handlers::ping_req_handler;

pub struct PingRequest;

impl ResponseType for PingRequest {
    type Item = ();
    type Error = Error;
}

pub struct PingTimeout;

impl ResponseType for PingTimeout {
    type Item = ();
    type Error = Error;
}

pub struct PingResponse(SpawnHandle);

impl ResponseType for PingResponse {
    type Item = ();
    type Error = Error;
}

impl<I: 'static, P: 'static> Handler<PingTimeout, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {

    fn handle(&mut self, msg: PingTimeout, ctx: &mut FramedContext<Self>) -> Response<Self, PingTimeout> {
        match self.status {
            Some(LoopStatus::Disconnected) | Some(LoopStatus::PendingError(_)) => Loop::empty(),
            _ => {
                self.status = Some(LoopStatus::PendingError(
                    Error::from(ProtoErrorKind::ResponseTimeout(PacketType::PingReq))));
                Loop::empty()
            }
        }
    }
}

impl<I: 'static, P: 'static> Handler<PingResponse, Canceled> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {

    fn handle(&mut self, msg: PingResponse, ctx: &mut FramedContext<Self>) -> Response<Self, PingResponse> {
        ctx.cancel_future(msg.0);
        self.timer = self.timer.or(Some(ctx.notify(PingRequest, self.keep_alive_dur())));
        Loop::empty()
    }
}

impl<I: 'static, P: 'static> Handler<PingRequest, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {

    fn handle(&mut self, msg: PingRequest, ctx: &mut FramedContext<Self>) -> Response<Self, PingRequest> {
        match self.status {
            Some(LoopStatus::Connected) |
            Some(LoopStatus::Disconnecting(_, DisconnectState::Waiting(_))) => {
                let packet = MqttPacket::ping_req_packet();
                let (tx, rx) = channel::<Result<ClientReturn>>();
                let timeout_hdl = ctx.notify(PingTimeout,
                                             Duration::from_secs(self.config.ping_timeout));
                let res = ping_req_handler((packet, tx), &mut self.data).and_then(|o| {
                    if let Some(p) = o {
                        ctx.send(p).map_err(|_| Error::from(ErrorKind::LoopError))
                    } else {
                        Ok(())
                    }
                });
                match res {
                    Ok(()) => {
                        ctx.add_future(rx.map(move |_| PingResponse(timeout_hdl)));
                        Loop::empty()
                    }
                    Err(e) => Loop::reply_error(e)
                }
            },
            _ => Loop::empty()
        }
    }
}
