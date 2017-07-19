use std::collections::HashMap;
use std::time::Duration;
use std::mem;

use ::futures::{Poll, Async};
use ::futures::future::{Future};
use ::futures::stream::{Stream, FuturesUnordered, futures_unordered};
use ::futures::sync::oneshot::{channel, Sender, Canceled};
use ::futures::sync::mpsc::{unbounded, UnboundedSender};
use ::tokio_io::{AsyncRead, AsyncWrite};
use ::tokio_core::reactor::{Handle, Timeout};
use ::futures_mutex::FutMutex;
use ::take::Take;

use ::errors::{Error, ErrorKind, Result, ResultExt};
use ::errors::proto::ErrorKind as ProtoErrorKind;
use ::persistence::Persistence;
use ::proto::{MqttPacket, PacketType};
use super::codec::MqttCodec;
use super::{
    BoxFuture,
    ClientQueue,
    ClientReturn,
    ClientRequestType,
    ClientRequest,
    MqttFramedWriter,
    MqttFramedReader,
    SubscriptionSender,
    SourceItem,
    SourceError,
    PublishState,
    OneTimeKey,
    TopicFilter,
    TimeoutType
};
use super::response::ResponseProcessor;
use super::request::RequestProcessor;

enum State {
    Disconnected(UnboundedSender<ClientRequest>),
    Running(UnboundedSender<ClientRequest>),
    Disconnecting(UnboundedSender<ClientRequest>),
    Closed
}

pub struct LoopClient {
    state: Take<State>
}

impl LoopClient {
    pub fn new(q: UnboundedSender<ClientRequest>) -> LoopClient {
        LoopClient {
            state: Take::new(State::Disconnected(q))
        }
    }

    fn send_request<F>(sender: UnboundedSender<ClientRequest>, req_ty: ClientRequestType,
        next_state: F) ->
        Result<(State, Result<BoxFuture<Result<ClientReturn>, Canceled>>)>
        where F: FnOnce(UnboundedSender<ClientRequest>) -> State {

            let (tx, rx) = channel::<Result<ClientReturn>>();
            let (init_tx, init_rx) = channel::<Result<()>>();
            let req = ClientRequest::new(init_tx, tx, req_ty);
            if let Err(e) = sender.send(req).chain_err(|| ErrorKind::LoopCommsError) {
                Ok((State::Closed, Err(e)))
            } else {
                if let Err(e) = init_rx.wait().map_err(|_| ErrorKind::LoopCommsError)? {
                    Ok((State::Closed, Err(e)))
                } else {
                    Ok((next_state(sender), Ok(rx.boxed())))
                }
            }
    }

    pub fn connect(&mut self, packet: MqttPacket, connect_timeout: u64) ->
        Result<BoxFuture<Result<ClientReturn>, Canceled>> {

        let (new_state, ret) = match self.state.take() {
            State::Disconnected(q) => {
                LoopClient::send_request(q, ClientRequestType::Connect(packet, connect_timeout),
                    |q| State::Running(q))?
            },
            State::Running(q) => (State::Running(q), Err(ErrorKind::AlreadyConnected.into())),
            State::Disconnecting(q) => (State::Disconnecting(q),
                Err(ErrorKind::ClientUnavailable.into())),
            State::Closed => (State::Closed, Err(ErrorKind::ClientUnavailable.into()))
        };

        self.state = Take::new(new_state);
        ret
    }

    pub fn request(&mut self, packet: MqttPacket) -> Result<BoxFuture<Result<ClientReturn>, Canceled>> {
        let (new_state, ret) = match self.state.take() {
            State::Running(q) => {
                LoopClient::send_request(q, ClientRequestType::Normal(packet),
                    |q| State::Running(q))?
            },
            _ => return Err(ErrorKind::ClientUnavailable.into())
        };

        self.state = Take::new(new_state);
        ret
    }

    pub fn disconnect(&mut self, timeout: Option<u64>) ->
        Result<BoxFuture<Result<ClientReturn>, Canceled>> {

        let (new_state, ret) = match self.state.take() {
            State::Running(q) => {
                LoopClient::send_request(q, ClientRequestType::Disconnect(timeout),
                    |q| State::Disconnecting(q))?
            },
            _ => return Err(ErrorKind::ClientUnavailable.into())
        };

        self.state = Take::new(new_state);
        ret
    }
}

pub struct LoopData<'p, I, P> where I: AsyncRead + AsyncWrite + 'static,
    P: 'p + Persistence {
    // Framed IO writer
    pub framed_write: MqttFramedWriter<I>,
    // Sink for app messages that come from old subs
    pub session_subs: Option<SubscriptionSender>,
    // Server publishing state tracker for QoS1,2 messages
    pub server_publish_state: HashMap<u16, PublishState<P>>,
    // Client publish state tracker for QoS1,2 messages
    pub client_publish_state: HashMap<u16, PublishState<P>>,
    // Map recievers of one-time requests
    pub one_time: HashMap<OneTimeKey, (MqttPacket, Sender<Result<ClientReturn>>)>,
    // Current subscriptions
    pub subscriptions: HashMap<String, (TopicFilter, SubscriptionSender)>,
    // Shared mutable Persistence cache
    // TODO: Avoid runtime borrow checking
    pub persistence: &'p mut P
}

/// ## Internal loop for the MQTT client.
///
/// This loop is designed to address the issue that an MQTT client acts as both a sender **and** a
/// reciever, thus needs to not only initiate requests, but respond to requests. We essentially
/// need a smaller loop inside the core reactor that can perform this behaviour for us. This loop
/// is designed to communicate to the outside world through queues; one queue to recieve commands
/// from the client, and multiple queues to either provide responses to one-shot requests like
/// CONNECT, DISCONNECT, PUBLISH, and PINGREQ, or provide a stream of requests for subscriptions.
///
/// The story for this loop goes like this.
///
/// ### Loop creation
///
/// Loop is created, returning a pair of objects. This first object, `LoopClient`, is a wrapper
/// object that exposes an API for sending either oneshot requests or streaming requests; this
/// will return futures that return a single value or a stream of values, respectively.
///
/// The second object, `Loop`, is a future designed to run indefinetely on the core loop. It
/// contains the logic that handles requests/responses from the `LoopClient`, performs PUBLISH
/// QoS acknowledgements, feeds subscriptions, and pings the server for liveliness. More about its
/// inner workings on the `Loop` docs.
///
/// The loop is composed of 3 sub loops: Response, Request and Timeout. Each does work is parallel
/// to drive the client.
///
/// ### Loop operates
///
/// The loop is spawned on the core event loop. It runs indefinetely until either one of these
/// events occurs. All subscription streams and in-flight requests will recieve a notification of
/// this event.
/// 1. The client disconnects from the server. Here we simply wait for an disconnect
///    acknowledgement, store the response in a oneshot, then finish.
/// 2. The client encounters a protocol error. The loop will alert all streams and any in-flight
///    one-shot requests. It will stay on and return errors for every operation
/// 3. The network connection fails. The loop will alert all streams and any in-flight
///    one-shot requests. It will stay on and return errors for every operation.
///
/// The loop will simultaneously poll for new requests and for new responses. It will perform the
/// appropriate action based on what which event source is returned.
///
/// ### LoopClient sends requests
///
/// The `LoopClient` supports 3 operations.
/// 1. One-off operations like DISCONNECT, PUBLISH, PINGREQ, UNSUBSCRIBE.
/// 2. Streaming PUBLISH using a sink.
/// 3. Streaming SUBSCRIBE using a stream.
/// These operations all send an enum on the in-queue to the loop. It also sends a queue for the
/// loop to return a value to the client.
///
/// ### Loop processes request
///
/// The loop processes the request and returns one of the following on the provided queue.
/// 1. A future that resolves when the server acknowledges the packet.
/// 2. A stream of requests matching a subscription.
///
/// The CONNECT operation follows the procedure for 1, but instead of a blank response, the future
/// returns a stream of packets that the client missed, if a existing session was found on the
/// server. If the client then re-subscribes to those topics, this stream won't recieve them; the
/// loop will instead forward them to the correct subscription stream.
///
/// ### Loop recieves a packet
///
/// When the loop recieves a packet, it will perform the correct logic to process the packet.
///
/// #### CONNACK, SUBACK, UNSUBACK, PINGRESP
///
/// The loop will alert the required future/stream
///
/// #### PUBLISH, PUBACK, PUBREC, PUBREL, PUBCOMP
///
/// The loop holds state machines for QoS1, QoS2 packets and will transparently go through the
/// acknowledgement process for them. The relavent future/stream will receive the final result.
///
/// ### Loop pings the server
///
/// Along with normal operation, the loop will send PINGREQ packets when the loop has been idle for
/// keep-alive seconds. A timeout is created and added to the list of waiting operations. If a
/// operation occurs that isn't the timeout, the timeout is dropped and a new one is setup instead.

pub struct Loop<'p, I, P>
    where I: AsyncRead + AsyncWrite + 'static, P: 'p + Persistence {
    // Keep Alive time
    keep_alive: u64,
    // Handle to the reactor
    handle: Handle,
    // Shared loop data
    data: FutMutex<LoopData<'p, I, P>>,
    // Current event sources
    sources: FuturesUnordered<BoxFuture<SourceItem<I>, SourceError<I>>>,
    // Status of the loop
    status: LoopStatus<Error>,
    // Tells us whether there are still in-flight QoS1/2 packets
    inflight: bool,
    // Holds the ID of the current timer
    timer_flag: Option<usize>
}

impl<'p, I, P> Loop<'p, I, P> where I: AsyncRead + AsyncWrite + 'static,
    P: 'p + Persistence {
    pub fn new(io: I, persist: &'p mut P, handle: Handle, keep_alive: u64, connect_timeout: u64) -> Result<(Loop<'p, I, P>, LoopClient)> {
        let (tx, rx) = unbounded::<ClientRequest>();
        let (fwrite, fread) = io.framed(MqttCodec).split();
        let client = LoopClient::new(tx.clone());
        let loop_data = FutMutex::new(LoopData {
            framed_write: fwrite,
            session_subs: None,
            server_publish_state: HashMap::new(),
            client_publish_state: HashMap::new(),
            one_time: HashMap::new(),
            subscriptions: HashMap::new(),
            persistence: persist
        });
        let res_fut = Loop::<'p, I, P>::response_wrapper(fread);
        let req_fut = Loop::<'p, I, P>::request_wrapper(rx.peekable());

        let sources = futures_unordered(vec![res_fut, req_fut]);
        let lp = Loop {
            keep_alive: keep_alive,
            handle: handle,
            status: LoopStatus::Disconnected,
            data: loop_data,
            sources: sources,
            inflight: false,
            timer_flag: None
        };
        Ok((lp, client))
    }

    fn request_wrapper(stream: ClientQueue) -> BoxFuture<SourceItem<I>, SourceError<I>> {
        stream.into_future()
        .map(|(i, s)| SourceItem::GotRequest(s, i))
        .map_err(|(_, s)| SourceError::GotRequest(s, ErrorKind::LoopCommsError.into()))
        .boxed()
    }

    fn response_wrapper(stream: MqttFramedReader<I>) -> BoxFuture<SourceItem<I>, SourceError<I>> {
        Box::new(stream.into_future()
        .map(|(i, s)| SourceItem::GotResponse(s, i))
        .map_err(|(e, s)| SourceError::GotResponse(s, e)))
    }

    fn make_timeout(timeout: Timeout, ty: TimeoutType) ->
        BoxFuture<SourceItem<I>, SourceError<I>> {

        timeout.map(|_| {
            SourceItem::Timeout(ty)
        }).map_err(|e| {
            SourceError::Timeout(e.into())
        }).boxed()
    }
}

impl<'p, I, P> Future for Loop<'p, I, P> where I: AsyncRead + AsyncWrite + 'static,
    P: 'p + Persistence, 'p: 'static {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let LoopStatus::Done = self.status {
                return Ok(Async::Ready(()))
            }

            let res = match self.sources.poll() {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(r)) => r,
                Err(e) => {
                    // Close the connection to the server
                    self.status = LoopStatus::PendingError(e.into());
                    continue;
                }
            };
            match res {
                Some(SourceItem::GotRequest(mut queue, request)) => {
                    let mut current_state = &mut self.status;
                    match *current_state {
                        LoopStatus::Connected => {
                            use super::ClientRequestType::*;

                            match request {
                                Some(ClientRequest{ty: Normal(packet), return_chan, inital_ret}) => {
                                    match queue.peek() {
                                        Ok(Async::Ready(Some(_))) => self.timer_flag = None,
                                        Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
                                            // Set the timeout flag
                                            let id = self.timer_flag.take()
                                                .map(|t| t + 1).unwrap_or(0);
                                            self.timer_flag = Some(id);
                                            // Make the timeout
                                            let timeout = Timeout::new(
                                                Duration::from_secs(self.keep_alive),
                                                &self.handle).ok();
                                            if let Some(timer) = timeout {
                                                let timeout_fut = Loop::<I,P>::make_timeout(
                                                    timer, TimeoutType::Ping(id));
                                                self.sources.push(timeout_fut);
                                            }
                                        },
                                        _ => {}
                                    }
                                    self.sources.push(Box::new(RequestProcessor::new(
                                        (packet, return_chan),
                                        self.data.clone())));
                                    let _ = inital_ret.send(Ok(()));
                                    *current_state = LoopStatus::Connected;
                                },
                                Some(ClientRequest{ty: Disconnect(time), return_chan, ..}) => {
                                    // Start a timeout for the disconnect
                                    let hdl = self.handle.clone();
                                    let timeout = time.and_then(|t| {
                                        Timeout::new(Duration::from_secs(t), &hdl).ok()
                                    });
                                    if let Some(timer) = timeout {
                                        let timeout_fut = Loop::<I,P>::make_timeout(timer,
                                            TimeoutType::Disconnect);
                                        self.sources.push(timeout_fut);
                                        // Move state to Disconnecting
                                        *current_state = LoopStatus::Disconnecting(
                                            DisconnectState::Waiting, return_chan);
                                    } else {
                                        *current_state = LoopStatus::Disconnecting(
                                            DisconnectState::Sending, return_chan);
                                    }
                                },
                                Some(ClientRequest{ty: _, return_chan, inital_ret}) => {
                                    let _ = inital_ret.send(
                                        Err(ErrorKind::LoopStateError.into()));
                                    return Ok(Async::Ready(()));
                                },
                                None => {
                                    *current_state = LoopStatus::PendingError(
                                        ErrorKind::LoopCommsError.into());
                                }
                            }
                        },
                        LoopStatus::PendingError(_) => {
                            let e = match mem::replace(current_state, LoopStatus::Done) {
                                LoopStatus::PendingError(e) => e,
                                _ => unreachable!()
                            };
                            if let Some(req) = request {
                                let _ = req.inital_ret.send(Err(e));
                            }
                            return Ok(Async::Ready(()));
                        },
                        _ => unreachable!()
                    };
                },
                Some(SourceItem::ProcessRequest(sent, more)) => {
                    self.inflight = more;
                    // Set the timeout flag
                    let id = self.timer_flag.take().map(|t| t + 1).unwrap_or(0);
                    self.timer_flag = Some(id);
                    // Make the timeout
                    let timeout = Timeout::new(Duration::from_secs(self.keep_alive),
                        &self.handle).ok();
                    if let Some(timer) = timeout {
                        let timeout_fut = Loop::<I,P>::make_timeout(timer, TimeoutType::Ping(id));
                        self.sources.push(timeout_fut);
                    }
                },
                Some(SourceItem::GotResponse(strm, packet)) => {
                    let mut current_state = &mut self.status;
                    match current_state {
                        &mut LoopStatus::Connected | &mut LoopStatus::Disconnecting(_, _) => {
                            if let Some(p) = packet {
                                self.sources.push(Loop::<I, P>::response_wrapper(strm));
                                self.sources.push(Box::new(ResponseProcessor::new(p,
                                    self.data.clone())));
                            } else {
                                *current_state = LoopStatus::PendingError(
                                    ErrorKind::UnexpectedDisconnect.into())
                            }
                        },
                        _ => unreachable!()
                    }
                },
                Some(SourceItem::ProcessResponse(sent, work)) => {
                    let current_state = &mut self.status;
                    match current_state {
                        &mut LoopStatus::Connected => {
                            self.inflight = work;
                            if sent {
                                self.timer_flag = None;
                            } else {
                                // Set the timeout flag
                                let id = self.timer_flag.take().map(|t| t + 1).unwrap_or(0);
                                self.timer_flag = Some(id);
                                // Make the timeout
                                let timeout = Timeout::new(Duration::from_secs(self.keep_alive),
                                    &self.handle).ok();
                                if let Some(timer) = timeout {
                                    let timeout_fut = Loop::<I,P>::make_timeout(timer, TimeoutType::Ping(id));
                                    self.sources.push(timeout_fut);
                                }
                            }
                        },
                        &mut LoopStatus::Disconnecting(ref mut state, ret) => {
                            // Have we finished our work?
                            match state
                            if work {
                                if sent {
                                    self.timer_flag = None;
                                } else {
                                    // Set the timeout flag
                                    let id = self.timer_flag.take().map(|t| t + 1).unwrap_or(0);
                                    self.timer_flag = Some(id);
                                    // Make the timeout
                                    let timeout = Timeout::new(Duration::from_secs(self.keep_alive),
                                        &self.handle).ok();
                                    if let Some(timer) = timeout {
                                        let timeout_fut = Loop::<I,P>::make_timeout(timer, TimeoutType::Ping(id));
                                        self.sources.push(timeout_fut);
                                    }
                                }
                            } else {
                                *state = DisconnectState::Sending;
                            }
                        },
                        _ => unreachable!()
                    }
                },
                Some(SourceItem::Timeout(t)) => {
                    match t {
                        TimeoutType::Connect => {
                            let mut current_state = &mut self.status;
                            match *current_state {
                                LoopStatus::Connecting(_) => {
                                    *current_state = LoopStatus::PendingError(
                                        ProtoErrorKind::ResponseTimeout(
                                            PacketType::Connect
                                        ).into()
                                    );
                                },
                                _ => {}
                            }
                        }
                        TimeoutType::Ping(id) => {
                            // Send a ping request if this timer is valid
                            if let Some(x) = self.timer_flag {
                                if x == id {
                                    let req = MqttPacket::ping_req_packet();
                                    let (tx, rx) = channel::<Result<ClientReturn>>();
                                    self.sources.push(Box::new(RequestProcessor::new((req, tx),
                                        self.data.clone())));
                                    self.sources.push(rx
                                        .map(|_| SourceItem::GotPingResponse)
                                        .map_err(|_| SourceError::GotPingResponse).boxed()
                                    );
                                    self.timer_flag = None;
                                }
                            }
                        },
                        TimeoutType::Disconnect => {
                            // Shutdown all the queue stuff

                        }
                    }
                },
                Some(SourceItem::GotPingResponse) => {
                    // Set the timeout flag
                    let id = self.timer_flag.take().map(|t| t + 1).unwrap_or(0);
                    self.timer_flag = Some(id);
                    // Make the timeout
                    let timeout = Timeout::new(Duration::from_secs(self.keep_alive),
                        &self.handle).ok();
                    if let Some(timer) = timeout {
                        let timeout_fut = Loop::<I,P>::make_timeout(timer, TimeoutType::Ping(id));
                        self.sources.push(timeout_fut);
                    }
                }
                None => {}
            };
            return Ok(Async::NotReady);
        }
    }
}

enum LoopStatus<E> {
    // Loop is disconnected from the server
    Disconnected,
    // Loop waits for either a conn_ack or the timeout
    Connecting(Sender<Result<ClientReturn>>),
    // Loop is connected and operates normally
    Connected,
    // Loop is in the process of disconnecting
    Disconnecting(DisconnectState, Sender<Result<ClientReturn>>),
    // Loop has an error that has not been reported to the client
    PendingError(E),
    // Loop is done.
    Done
}

enum DisconnectState {
    // Waiting for works to finish
    Waiting,
    // Sending the disconnect packet
    Sending,
    // Closing the connection
    Closing
}
