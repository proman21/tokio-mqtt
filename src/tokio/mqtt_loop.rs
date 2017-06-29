use std::collections::HashMap;
use std::time::Duration;
use ::futures::{Poll, Async};
use ::futures::future::{Future, BoxFuture};
use ::futures::stream::{Stream, FuturesUnordered, futures_unordered};
use ::futures::sync::oneshot::{channel, Sender, Canceled};
use ::futures::sync::mpsc::{unbounded, UnboundedSender};
use ::tokio_io::{AsyncRead, AsyncWrite};
use ::tokio_core::reactor::{Handle, Timeout};
use ::futures_mutex::FutMutex;
use ::take::Take;

use ::errors::{Error, ErrorKind, Result, ResultExt};
use ::persistence::Persistence;
use ::proto::MqttPacket;
use super::codec::MqttCodec;
use super::{
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
    Running(UnboundedSender<ClientRequest>),
    Disconnecting,
    Closed
}

pub struct LoopClient {
    state: Take<State>
}

impl LoopClient {
    pub fn new(q: UnboundedSender<ClientRequest>) -> LoopClient {
        LoopClient {
            state: Take::new(State::Running(q))
        }
    }
    pub fn request(&mut self, packet: MqttPacket) -> Result<BoxFuture<Result<ClientReturn>, Canceled>> {
        match self.state.take() {
            State::Running(q) => {
                let (tx, rx) = channel::<Result<ClientReturn>>();
                let (init_tx, init_rx) = channel::<Result<()>>();
                let req = ClientRequest::new(init_tx, tx, ClientRequestType::Normal(packet));
                if let Err(e) = q.send(req).chain_err(|| ErrorKind::LoopCommsError) {
                    self.state = Take::new(State::Closed);
                    return Err(e)
                };
                if let Err(e) = init_rx.wait().map_err(|_| ErrorKind::LoopCommsError)? {
                    self.state = Take::new(State::Closed);
                    return Err(e)
                } else {
                    self.state = Take::new(State::Running(q));
                    return Ok(rx.boxed())
                }
            },
            _ => return Err(ErrorKind::ClientUnavailable.into())
        }
    }

    pub fn disconnect(&mut self, timeout: Option<u64>) -> Result<BoxFuture<Result<ClientReturn>, Canceled>> {
        match self.state.take() {
            State::Running(q) => {
                let (tx, rx) = channel::<Result<ClientReturn>>();
                let (init_tx, init_rx) = channel::<Result<()>>();
                let req = ClientRequest::new(init_tx, tx, ClientRequestType::Disconnect(timeout));
                if let Err(e) = q.send(req).chain_err(|| ErrorKind::LoopCommsError) {
                    self.state = Take::new(State::Closed);
                    return Err(e)
                };
                if let Err(e) = init_rx.wait().map_err(|_| ErrorKind::LoopCommsError.into()).map(|r| r) {
                    self.state = Take::new(State::Closed);
                    return Err(e)
                } else {
                    self.state = Take::new(State::Disconnecting);
                    return Ok(rx.boxed());
                }
            },
            _ => return Err(ErrorKind::ClientUnavailable.into())
        }
    }
}

pub struct LoopData<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
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
    pub persistence: P,
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

pub struct Loop<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    // Keep Alive time
    keep_alive: u64,
    // Handle to the reactor
    handle: Handle,
    // Shared loop data
    data: FutMutex<LoopData<I, P>>,
    // Current event sources
    sources: FuturesUnordered<BoxFuture<SourceItem<I>, SourceError<I>>>,
    // Request sender
    req_sender: UnboundedSender<ClientRequest>,
    // Status of the loop
    status: Take<LoopStatus<Error>>,
    // Holds the ID of the current timer
    timer_flag: Option<usize>
}

impl<I, P> Loop<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence + Send + 'static {
    pub fn new(io: I, persist: P, handle: Handle, keep_alive: u64) -> Result<(Loop<I, P>, LoopClient)> {
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
        let res_fut = Loop::<I, P>::response_wrapper(fread);
        let req_fut = Loop::<I, P>::request_wrapper(rx.peekable());

        let tout_res = Timeout::new(Duration::from_secs(keep_alive), &handle)?;
        let tout_fut = Loop::<I,P>::make_timeout(tout_res, TimeoutType::Ping(0));

        let sources = futures_unordered(vec![res_fut.boxed(), req_fut, tout_fut.boxed()]);
        let lp = Loop {
            keep_alive: keep_alive,
            handle: handle,
            status: Take::new(LoopStatus::Running),
            data: loop_data,
            req_sender: tx,
            sources: sources,
            timer_flag: Some(0)
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
        stream.into_future()
        .map(|(i, s)| SourceItem::GotResponse(s, i))
        .map_err(|(e, s)| SourceError::GotResponse(s, e))
        .boxed()
    }

    fn make_timeout(timeout: Timeout, ty: TimeoutType) -> BoxFuture<SourceItem<I>, SourceError<I>> {
        timeout.map(|_| {
            SourceItem::Timeout(ty)
        }).map_err(|e| {
            SourceError::Timeout(e.into())
        }).boxed()
    }
}

impl<I, P> Future for Loop<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence + Send + 'static {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let LoopStatus::Done = self.status.take() {
                self.status = Take::new(LoopStatus::Done);
                return Ok(Async::NotReady)
            }

            let res = match self.sources.poll() {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(r)) => r,
                Err(e) => {
                    // Close the connection to the server
                    self.status = Take::new(LoopStatus::Error(e.into()));
                    continue;
                }
            };
            match res {
                Some(SourceItem::GotResponse(strm, packet)) => {
                    self.status = match self.status.take() {
                        s @ LoopStatus::Running | s @ LoopStatus::Disconnecting(_) => {
                            if let Some(p) = packet {
                                self.sources.push(Loop::<I, P>::response_wrapper(strm));
                                self.sources.push(ResponseProcessor::new(p,
                                    self.data.clone()).boxed());
                                Take::new(s)
                            } else {
                                Take::new(LoopStatus::Error(ErrorKind::UnexpectedDisconnect.into()))
                            }
                        },
                        _ => unreachable!()
                    };
                },
                Some(SourceItem::ProcessResponse(sent)) => {
                    self.status = match self.status.take() {
                        s @ LoopStatus::Running | s @ LoopStatus::Disconnecting(_) => {
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
                            Take::new(s)
                        },
                        _ => unreachable!()
                    };
                }
                Some(SourceItem::GotRequest(mut queue, request)) => {
                    self.status = match self.status.take() {
                        LoopStatus::Running => {
                            if let Some(req) = request {
                                let _ = req.inital_ret.send(Ok(()));
                                match req.ty {
                                    ClientRequestType::Normal(packet) => {
                                        match queue.peek() {
                                            Ok(Async::Ready(Some(_))) => self.timer_flag = None,
                                            Ok(Async::Ready(None)) | Ok(Async::NotReady) => {
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
                                            _ => {}
                                        }
                                        self.sources.push(RequestProcessor::new(
                                            (packet, req.return_chan),
                                            self.data.clone()).boxed());
                                        Take::new(LoopStatus::Running)
                                    },
                                    ClientRequestType::Disconnect(time) => {
                                        // Start a timeout for the disconnect
                                        let timeout = time.and_then(|t| Timeout::new(Duration::from_secs(t), &self.handle).ok());
                                        if let Some(timer) = timeout {
                                            let timeout_fut = Loop::<I,P>::make_timeout(timer, TimeoutType::Disconnect);
                                            self.sources.push(timeout_fut);
                                        }
                                        // Move state to Disconnecting
                                        Take::new(LoopStatus::Disconnecting(req.return_chan))
                                    }
                                }
                            } else {
                                Take::new(LoopStatus::Error(ErrorKind::LoopCommsError.into()))
                            }
                        },
                        LoopStatus::Error(e) => {
                            if let Some(req) = request {
                                let _ = req.inital_ret.send(Err(e));
                            }
                            Take::new(LoopStatus::Done)
                        }
                        _ => unreachable!()
                    };
                },
                Some(SourceItem::ProcessRequest(sent)) => {
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
                Some(SourceItem::Timeout(t)) => {
                    // Check the type of the timeout, either Ping or Disconnect
                    match t {
                        TimeoutType::Ping(id) => {
                            // Send a ping request if this timer is valid
                            match self.timer_flag {
                                Some(x) if x == id => {

                                },
                                _ => {}
                            }
                        },
                        TimeoutType::Disconnect => {
                            // Shutdown all the queue stuff
                        }
                    }
                },
                None => {}
            };
            return Ok(Async::NotReady);
        }
    }
}

enum LoopStatus<E> {
    Running,
    Disconnecting(Sender<Result<ClientReturn>>),
    Error(E),
    Closing,
    Done
}
