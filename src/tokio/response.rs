use std::collections::hash_map::Entry;
use std::result;

use ::tokio_io::{AsyncRead, AsyncWrite};
use ::futures::{Poll, Async, Stream, Future};
use ::futures::sync::mpsc::{unbounded};
use ::futures::unsync::mpsc::UnboundedSender;
use ::futures_mutex::{FutMutex, FutMutexGuard};

use ::persistence::Persistence;
use ::proto::{MqttPacket, QualityOfService, ConnectReturnCode, ConnectAckFlags};
use ::errors::{Error, ErrorKind};
use ::errors::proto::{ErrorKind as ProtoErrorKind};
use ::types::SubItem;
use super::mqtt_loop::LoopData;
use super::inbound_handlers::ResponseHandler;
use super::{
    SourceError,
    ClientReturn,
    OneTimeKey,
    MqttFramedReader,
    LoopRequest
};

enum State<'p> {
    Receiving,
    Processing(ResponseHandler<'p>)
}

/// This type will take a packet from the server and process it, returning the stream it came from
pub struct ResponseProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite + 'static, P: 'p + Persistence {
    state: Option<State<'p>>,
    stream: MqttFramedReader<I>,
    req_chan: UnboundedSender<LoopRequest>,
    data_lock: FutMutex<LoopData<'p, P>>
}

impl<'p, I, P> ResponseProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite + 'static, P: 'p + Persistence {
    pub fn new(packet: MqttPacket, stream: MqttFramedReader<I>,
        req_chan: UnboundedSender<LoopRequest>, data_lock: FutMutex<LoopData<'p, P>>) ->
        ResponseProcessor<'p, I, P> {

        ResponseProcessor {
            state: Some(State::Receiving),
            stream,
            req_chan,
            data_lock,
        }
    }

    fn process_conn_ack(mut data: FutMutexGuard<LoopData<'p, P>>, packet: MqttPacket) -> result::Result<Option<MqttPacket>, SourceError<I>> {
        // Check if we are awaiting a CONNACK
        let (_, client) = match data.one_time.entry(OneTimeKey::Connect) {
            Entry::Vacant(v) => {
                return Err(SourceError::ProcessResponse(ErrorKind::from(
                        ProtoErrorKind::UnexpectedResponse(packet.ty.clone())
                ).into()));
            },
            Entry::Occupied(o) => o.remove()
        };
        // Validate connect return code
        let crc = packet.headers.get::<ConnectReturnCode>().unwrap();
        if crc.is_ok() {
            // If session is not clean, setup a stream.
            let sess = packet.headers.get::<ConnectAckFlags>().unwrap();
            if sess.is_clean() {
                let _ = client.send(Ok(ClientReturn::Onetime(Some(packet))));
            } else {
                let (tx, rx) = unbounded::<SubItem>();
                let custom_rx = rx.map_err(|_| {
                    Error::from(ErrorKind::LoopCommsError)
                });
                data.session_subs = Some(tx);
                let _ = client.send(Ok(ClientReturn::Ongoing(vec![
                    Ok((custom_rx.boxed(), QualityOfService::QoS0))
                ])));
            }
            Ok(None)
        } else {
            return Err(SourceError::ProcessResponse(ErrorKind::from(
                ProtoErrorKind::ConnectionRefused(*crc)).into()
            ))
        }
    }
}

impl<'p, I, P> Future for ResponseProcessor<'p, I, P>
    where I: AsyncRead + AsyncWrite + 'static, P: 'p + Persistence {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::State::*;

        loop { match self.state {
            Some(Receiving) => {
                let read = match try_ready!(self.stream.poll()) {
                    Some(res) => res,
                    None => return Err(ErrorKind::UnexpectedDisconnect.into())
                };

                read.validate()?;

                self.state = Some(State::Processing(ResponseHandler::new(read,
                    self.req_chan.clone(), self.data_lock.clone())))
            },
            Some(Processing(_)) => {
                let mut handler = match self.state.take() {
                    Some(Processing(h)) => h,
                    _ => unreachable!()
                };
                match handler.poll() {
                    Ok(Async::Ready(_)) => self.state = Some(Receiving),
                    Ok(Async::NotReady) => {
                        self.state = Some(Processing(handler));
                        return Ok(Async::NotReady)
                    },
                    Err(e) => return Err(e)
                }
            },
            None => unreachable!()
        };}
    }
}
