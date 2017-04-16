use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ::futures::{Poll, Async};
use ::futures::future::{Future, BoxFuture, select_all};
use ::futures::stream::Stream;
use ::futures::sync::oneshot::{channel, Sender, Canceled};
use ::futures::sync::mpsc::{unbounded, UnboundedSender};
use ::tokio_io::{AsyncRead, AsyncWrite};

use ::errors::{Error, ErrorKind, Result, ResultExt};
use ::persistence::Persistence;
use ::proto::MqttPacket;
use super::codec::MqttCodec;
use super::{ClientReturn, MqttFramedWriter, SubscriptionSender, SourceTag,
    SourceItem, SourceError, PublishFlow, OneTimeKey};
use super::response::ResponseProcessor;

pub struct LoopClient {
    queue: UnboundedSender<(MqttPacket, Sender<Result<ClientReturn>>)>
}

impl LoopClient {
    pub fn new(q: UnboundedSender<(MqttPacket, Sender<Result<ClientReturn>>)>) -> LoopClient {
        LoopClient {
            queue: q
        }
    }
    pub fn request(&self, packet: MqttPacket) -> Result<BoxFuture<Result<ClientReturn>, Canceled>> {
        let (tx, rx) = channel::<Result<ClientReturn>>();
        self.queue.send((packet, tx)).chain_err(|| ErrorKind::LoopCommsError)?;
        Ok(rx.boxed())
    }
}

pub struct LoopData<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    // Framed IO writer
    pub framed_write: MqttFramedWriter<I>,
    // Sink for app messages that come from old subs
    pub session_subs: Option<SubscriptionSender>,
    // Server publishing state tracker for QoS1,2 messages
    pub server_publish_state: HashMap<u16, PublishFlow<P>>,
    // Client publish state tracker for QoS1,2 messages
    pub client_publish_state: HashMap<u16, PublishFlow<P>>,
    // Map recievers of one-time requests
    pub one_time: HashMap<OneTimeKey, (MqttPacket, Sender<Result<ClientReturn>>)>,
    // Current subscriptions
    pub subscriptions: HashMap<String, SubscriptionSender>,
    // Shared mutable Persistence cache
    pub persistence: Arc<Mutex<P>>,
}

/// Internal loop for the MQTT client.
///
/// This loop is designed to address the issue that an MQTT client acts as both a sender **and** a
/// reciever, thus needs to not only initiate requests, but respond to requests. We essentially
/// need a smaller loop inside the core loop that can perform this behaviour for us. This loop is
/// designed to communicate to the outside world through queues; one queue to recieve commands from
/// the client, and multiple queues to either provide responses to one-shot requests like CONNECT,
/// DISCONNECT, PUBLISH, and PINGREQ, or provide a stream of requests for subscriptions.
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
    data: Arc<Mutex<LoopData<I, P>>>,
    // Current event sources
    sources: HashMap<SourceTag, BoxFuture<SourceItem<I>, SourceError>>,
    status: LoopStatus<Error>
}

impl<I, P> Loop<I, P> where I: AsyncRead + AsyncWrite + Send + 'static,
    P: Persistence + Send + 'static {
    pub fn new(io: I, persist: Arc<Mutex<P>>) -> (Loop<I, P>, LoopClient) {
        let (tx, rx) = unbounded::<(MqttPacket, Sender<Result<ClientReturn>>)>();
        let (fwrite, fread) = io.framed(MqttCodec).split();
        let client = LoopClient::new(tx);
        let loop_data = Arc::new(Mutex::new(LoopData {
            framed_write: fwrite,
            session_subs: None,
            server_publish_state: HashMap::new(),
            client_publish_state: HashMap::new(),
            one_time: HashMap::new(),
            subscriptions: HashMap::new(),
            persistence: persist
        }));
        let res_fut = ResponseProcessor::new(fread.into_future(), loop_data.clone());
        let mut sources = HashMap::new();
        let _ = sources.insert(SourceTag::Response, res_fut.boxed());
        let lp = Loop {
            status: LoopStatus::Running,
            data: loop_data,
            sources: sources
        };
        (lp, client)
    }
}

impl<I, P> Future for Loop<I, P> where I: AsyncRead + AsyncWrite + Send + 'static, P: Persistence {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let res = match select_all(self.sources.values_mut()).poll() {
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Ok(Async::Ready(r)) => Ok(r),
                Err(e) => Err(e)
            };
            match res {
                Ok((SourceItem::Response(strm), _, _)) => unimplemented!(),
                Ok((SourceItem::Request(s), _, _)) => unimplemented!(),
                Ok((SourceItem::Timeout(_), _, _)) => unimplemented!(),
                Err(e) => unimplemented!()
            };
        }
    }
}

enum LoopStatus<E> {
    Running,
    Error(E)
}
