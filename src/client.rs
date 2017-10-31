use std::default::Default;

use ::tokio_io::{AsyncRead, AsyncWrite};
use ::bytes::{Bytes};
use ::actix::{SyncAddress, Arbiter};

use ::proto::*;
use ::types::{BoxMqttFuture, SubscriptionStream as SubStream};
use ::errors::{Result as MqttResult};
use ::persistence::Persistence;

pub struct ClientConfig {
    pub connect_timeout: u64,
    pub ping_timeout: u64,
    pub keep_alive: u16,
    pub version: ProtoLvl,
    pub lwt: Option<(String, QualityOfService, bool, Bytes)>,
    pub creds: Credentials<String>,
    pub clean: bool,
    pub client_id: Option<String>
}

impl Default for ClientConfig {
    fn default() -> ClientConfig {
        ClientConfig {
            connect_timeout: 30,
            ping_timeout: 10,
            keep_alive: 0,
            version: ProtoLvl::V3_1_1,
            lwt: None,
            creds: None,
            clean: true,
            client_id: None
        }
    }
}

impl ClientConfig {
    /// Returns a client configuration with some defaults set.
    ///  - Connect Timeout: 30 seconds
    ///  - Keep Alive: 0 (off)
    ///  - Version: MQTT 3.1.1
    ///  - Last Will and Testament: None
    ///  - Clean Session: true
    ///  - Client ID: empty
    pub fn new() -> ClientConfig {
        ClientConfig::default()
    }

    /// Specify how long the client should wait for the server to acknowledge a connection request.
    /// 0 means wait indefinitely.
    pub fn connect_timeout(&mut self, t: u64) -> &mut ClientConfig {
        self.connect_timeout = t;
        self
    }

    /// Specify how long the client should wait for a ping response.
    /// 0 means wait indefinitely.
    pub fn ping_timeout(&mut self, t: u64) -> &mut ClientConfig {
        self.ping_timeout = t;
        self
    }

    /// Sets the keep alive value of the connection. If no response is recieved in `k * 1.5`
    /// seconds, the server will treat the client as disconnected. The client will send pings to
    /// the server every `k` seconds when idle.
    pub fn keep_alive(&mut self, k: u16) -> &mut ClientConfig {
        self.keep_alive = k;
        self
    }

    /// Specify which version of the MQTT protocol to use when communicating with the server.
    pub fn version(&mut self, v: ProtoLvl) -> &mut ClientConfig {
        self.version = v;
        self
    }

    /// Sets the Last Will and Testament message. This is stored by the server and sent to the
    /// specified topic in the event that:
    ///  - The network connection fails
    ///  - The client fails to communicate before the keep-alive time expires
    ///  - The client closes the connection without sending a disconnect message
    ///  - A protocol error occurs
    pub fn last_will(&mut self, t: String, q: QualityOfService, r: bool,
        m: &[u8]) -> &mut ClientConfig {
        self.lwt = Some((t, q, r, Bytes::from(m)));
        self
    }

    /// Specify the credentials the client will use when connecting to the server.
    pub fn credentials(&mut self, user: String, pass: Option<String>) -> &mut ClientConfig {
        self.creds = Some((user, pass));
        self
    }

    /// Specify whether the server should not treat this connection as a clean session.
    pub fn unclean_session(&mut self) -> &mut ClientConfig {
        self.clean = true;
        self
    }

    /// Set the ID used to identify this client to the server. If you haven't called the
    /// `clean_session` method and have a session stored on the server, you must set this value to
    /// the ID used in past sessions.
    pub fn client_id(&mut self, c: String) -> &mut ClientConfig {
        self.client_id = Some(c);
        self
    }
}

pub struct Client<P> where P: Persistence {
    actor_address: SyncAddress<Arbiter>,
    persistence: P
}

impl<P> Client<P> where P: Persistence {
    /// Return an empty configuration object. See `ClientConfig` for instructions on how to use.
    pub fn config() -> ClientConfig {
        ClientConfig::new()
    }

    /// Create a new client with the given configuration.
    ///
    /// `p` is an object that implements `Persistence` that holds in-flight packets for QoS1 and
    /// QoS2 publishing; this enables the client to maintain session across connections.
    pub fn new(p: P) -> Client<P> where P: Persistence {
        let arbiter = Arbiter::new(Some(String::from("mqtt")));
        Client {
            actor_address: arbiter,
            persistence: p
        }
    }

    /// Create a client using a particular actor system. Useful if you plan to use the client in
    /// an actor yourself.
    pub fn new_with_system(p: P, address: SyncAddress<Arbiter>) -> Client<P> where P: Persistence {
        Client {
            actor_address: address,
            persistence: p
        }
    }

    /// Starts an MQTT session with the provided configuration.
    /// `io` should be some network socket that provides an ordered, lossless stream of bytes.
    /// TCP, TLS and Websockets are all supported standards, however you may choose any protocol
    /// that fits the above criteria. The client will try to be well-behaved without this
    /// guarantee, but can not guarantee QoS compliance or good service.
    ///
    /// Note that the client takes an owned type, so that it may follow protocol conformance and
    /// disconnect the network connection when needed.
    /// **Please don't give this method a clone of a connection**. The client is expected to own a
    /// unique value that it can manipulate at will without disrupting other references to the
    /// underlying socket.
    ///
    /// If the server has a previous session with this client, and Clean Session has been set to
    /// false, the returned stream will contain all the messages that this client missed, based on
    /// its previous subscriptions.
    ///
    /// `config` provides options to configure the client.
    pub fn connect<'p, I>(&'p mut self, io: I, cfg: &ClientConfig) -> MqttResult<Option<SubStream>>
    where I: AsyncRead + AsyncWrite + 'static, P: Persistence {
        unimplemented!()
        // Setup a continual loop. This loop handles all the nitty gritty of reciving and
        // dispatching packets from the server. It essentially multiplexes packets to the correct
        // destination. Designed to run constantly on a Core loop, unless an error occurs.

        // // Prepare a connect packet to send using the provided values
        // let lwt = if let Some((ref t, ref q, ref r, ref m)) = cfg.lwt {
        //     let topic = MqttString::from_str(&t)?;
        //     Some(LWTMessage::new(topic, q.clone(), *r, m.clone()))
        // } else {
        //     None
        // };
        // let client_id = if let Some(ref cid) = cfg.client_id {
        //     Some(MqttString::from_str(&cid)?)
        // } else {
        //     None
        // };
        // let cred = if let Some((ref user, ref p)) = cfg.creds {
        //     let pwd = if let &Some(ref pass) = p {
        //         Some(MqttString::from_str(pass)?)
        //     } else {
        //         None
        //     };
        //     Some((MqttString::from_str(&user)?, pwd))
        // } else {
        //     None
        // };
        // let connect = MqttPacket::connect_packet(cfg.version, lwt, cred, cfg.clean,
        //     cfg.keep_alive, client_id);
        // // Send packet
        // let res_fut = client.request(connect)?;
        // // Wait for acknowledgement
        // let res = res_fut.wait().chain_err(|| ErrorKind::LoopAbortError)?;
        //
        //
        // match res? {
        //     ClientReturn::Onetime(_) => Ok(None),
        //     ClientReturn::Ongoing(mut subs) => {
        //         match subs.pop() {
        //             Some(Ok((s, _))) =>  Ok(Some(s.boxed())),
        //             _ => unreachable!()
        //         }
        //     },
        // }
    }

    /// Issues a disconnect packet to the server and closes the network connection. The client will
    /// wait for all QoS1 and QoS2 messages to be fully processed, unless `timeout` is specified,
    /// in which case the client will only wait until the timeout value has passed. The future
    /// returned will resolve to a bool; true means all packets were processed before disconnect,
    /// false meaning the timeout occurred before work could finish. All streams will still recieve
    /// packets until the the disconnect packet is issued
    pub fn disconnect(&mut self, timeout: Option<u64>) -> BoxMqttFuture<bool> {
        unimplemented!()
    }

    /// Publish a message to a particular topic with a particular QoS level. Returns a future that
    /// resolves when the publish completes.
    ///
    /// If you specified an encoder, `msg` will be whatever type you specified as `Encoder::Item`.
    /// The returned future will error if the encoding fails.
    pub fn publish(&mut self, topic: String, qos: QualityOfService, retain: bool, msg: Bytes) -> BoxMqttFuture<()> {
        unimplemented!()
    }

    /// Subscribe to a particular topic filter. This returns a Future which resolves to a `Stream`
    /// of messages that match the provided filter; the string provided will contain the topic
    /// string the subscription matched on. The stream will stop when `unsubscribe` is called with
    /// the matching topic filter or when the client disconnects.
    pub fn subscribe(&mut self, topic: String, qos: QualityOfService) -> BoxMqttFuture<SubStream> {
        unimplemented!()
    }

    /// Subscribes to multiple topic filters at once. This returns a `Vec` of `Future`'s that
    /// resolve to `Stream`'s of messages matching the corresponding topic filters. The streams
    /// will stop when `unsubscribe` is called with the matching topic filter or the client
    /// disconnects.
    pub fn subscribe_many(&mut self, subscriptions: Vec<(String, QualityOfService)>) ->  Vec<BoxMqttFuture<SubStream>>{
        unimplemented!()
    }

    /// Unsubscribe from a topic, causing the associated stream to terminate. Returns a future
    /// that resolves when acknowledged.
    pub fn unsubscribe(&mut self, topic: String) -> BoxMqttFuture<()> {
        unimplemented!()
    }

    /// Unsubscribe from multiple topics, causing the associated streams to terminate. Returns a
    /// future that resolves when acknowledged.
    pub fn unsubscribe_many(&mut self, topics: Vec<String>) -> BoxMqttFuture<()> {
        unimplemented!()
    }

    /// Ping the server to check it's still available. Returns a future that resolves when the
    /// server responds.
    ///
    /// The client will pause automatic pinging while this request is processed. This means that
    /// if the packet is lost in transit, and the server doesn't respond within the keep alive
    /// window, the client will assume the server or connection is down and will disconnect,
    /// producing an error in the future. Be aware that this scenario will trigger the sending of
    /// the Last Will and Testament message specified by this client.
    pub fn ping(&mut self) -> BoxMqttFuture<()> {
        unimplemented!()
    }
}
