use actix::prelude::*;
use tokio_io::{AsyncRead, AsyncWrite};
use futures::prelude::*;
use futures::sync::mpsc::unbounded;

use errors::{Error, ErrorKind, Result};
use errors::proto::ErrorKind as ProtoErrorKind;
use proto::{MqttPacket, PacketType, QualityOfService, ConnectReturnCode, ConnectAckFlags};
use super::{Loop, ClientReturn, SubItem};
use super::ping::PingRequest;
use super::mqtt_loop::{LoopStatus, DisconnectState};
use persistence::Persistence;
use backend::inbound_handlers::*;

impl<I: 'static, P: 'static> StreamHandler<MqttPacket, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {
}

impl ResponseType for MqttPacket {
    type Item = ();
    type Error = Error;
}

impl<I: 'static, P: 'static> Handler<MqttPacket, Error> for Loop<I, P>
    where I: AsyncRead + AsyncWrite, P: Persistence {

    fn handle(&mut self, msg: MqttPacket, ctx: &mut FramedContext<Self>) -> Response<Self, MqttPacket> {
        use self::LoopStatus::*;
        use self::PacketType::*;

        let res = match self.status {
            Some(Connected) => {
                match msg.ty {
                    PingResp => ping_resp_handler(&mut self.data),
                    PubAck => pub_ack_handler(msg, &mut self.data),
                    PubComp => pub_comp_handler(msg, &mut self.data),
                    PubRec => pub_rec_handler(msg, &mut self.data),
                    PubRel => pub_rel_handler(msg, &mut self.data),
                    Publish => publish_handler(msg, &mut self.data),
                    SubAck => sub_ack_handler(msg, &mut self.data),
                    UnsubAck => unsub_ack_handler(msg, &mut self.data),
                    _ => unreachable!()
                }
            },
            Some(Connecting(_, _)) => {
                match msg.ty {
                    ConnAck => {
                        let (timer, client) = match self.status.take() {
                            Some(Connecting(t, c)) => (t, c),
                            _ => unreachable!()
                        };

                        if let Some(hdl) = timer {
                            ctx.cancel_future(hdl);
                        }

                        let crc = msg.headers.get::<ConnectReturnCode>().unwrap();
                        if crc.is_ok() {
                            // If session is not clean, setup a stream.
                            let sess = msg.headers.get::<ConnectAckFlags>().unwrap();
                            if sess.is_clean() {
                                let _ = client.send(Ok(ClientReturn::Onetime(Some(msg))));
                            } else {
                                let (tx, rx) = unbounded::<Result<SubItem>>();
                                self.data.session_subs = Some(tx);
                                let _ = client.send(Ok(ClientReturn::Ongoing(vec![
                                    Ok((rx.into(), QualityOfService::QoS0))
                                ])));
                            }
                            self.status = Some(Connected);
                        } else {
                            let _ = client.send(Err(ErrorKind::from(
                                ProtoErrorKind::ConnectionRefused(*crc)).into()));
                            self.status = Some(Disconnected);
                        }
                    },
                    _ => {}
                };
                Ok(None)
            },
            Some(Disconnecting(_, DisconnectState::Waiting(_))) => {
                match msg.ty {
                    PingResp => ping_resp_handler(&mut self.data),
                    PubAck => pub_ack_handler(msg, &mut self.data),
                    PubComp => pub_comp_handler(msg, &mut self.data),
                    PubRec => pub_rec_handler(msg, &mut self.data),
                    PubRel => pub_rel_handler(msg, &mut self.data),
                    SubAck => sub_ack_handler(msg, &mut self.data),
                    UnsubAck => unsub_ack_handler(msg, &mut self.data),
                    _ => Ok(None)
                }
                // TODO: Check if there are no more QoS flows to handle and schedule a disconnect
            },
            Some(PendingError(_)) => Ok(None),
            _ => Ok(None)
        };
        match res {
            Ok(opt) => {
                if let Some(Err(_)) = opt.map(|p| ctx.send(p)) {
                    Loop::reply_error(ErrorKind::LoopError.into())
                } else {
                    self.timer = self.timer.map(|h| {
                        ctx.cancel_future(h);
                        ctx.notify(PingRequest, self.keep_alive_dur())
                    });
                    Loop::empty()
                }
            },
            Err(e) => Loop::reply_error(e)
        }
    }
}
