mod codec;
mod mqtt_loop;
mod response;
mod request;

pub use self::mqtt_loop::{Loop, LoopClient};
pub use self::codec::MqttCodec;

use std::ops::Deref;

use ::tokio_io::codec::Framed;
use ::futures::stream::{SplitStream, SplitSink, Peekable};
use ::futures::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use ::futures::sync::oneshot::Sender;
use ::regex::{escape, Regex};

use ::errors::{Result, Error, ErrorKind, ResultExt};
use ::proto::{MqttPacket, QualityOfService};
use ::types::{SubItem, BoxMqttStream};
use ::persistence::Persistence;

type MqttFramedReader<I> = SplitStream<Framed<I, MqttCodec>>;
type MqttFramedWriter<I> = SplitSink<Framed<I, MqttCodec>>;
type SubscriptionSender = UnboundedSender<Result<SubItem>>;
type ClientQueue = Peekable<UnboundedReceiver<ClientRequest>>;

pub enum TimeoutType {
    Ping(usize),
    Disconnect
}

pub enum ClientReturn {
    Onetime(Option<MqttPacket>),
    Ongoing(Vec<Result<(BoxMqttStream<Result<SubItem>>, QualityOfService)>>)
}

pub enum ClientRequest {
    Normal(MqttPacket, Sender<Result<ClientReturn>>),
    Disconnect(MqttPacket, Sender<Result<ClientReturn>>, Option<u64>)
}

/// These types act like tagged future items/errors, allowing the loop to know which future has
/// returned. This simplifies the process of handling sources.
pub enum SourceItem<I> {
    Response(MqttFramedReader<I>),
    Request(ClientQueue, Option<(Option<u64>, Sender<Result<ClientReturn>>)>),
    Timeout(TimeoutType)
}

pub enum SourceError {
    Response(Error),
    Request(Error),
    Timeout(Error)
}

impl Deref for SourceError {
    type Target = Error;

    fn deref(&self) -> &Self::Target {
        match *self {
            SourceError::Response(ref e) => e,
            SourceError::Request(ref e) => e,
            SourceError::Timeout(ref e) => e
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
pub enum OneTimeKey {
    Connect,
    PingReq,
    Subscribe(u16),
    Unsubscribe(u16)
}

/// ## QoS1
/// ### Server-sent publish
/// 1. Receive publish
/// 2. Send acknowledgement
/// ### Client-sent publish
/// 1. Send packet, start at Sent
/// 2. Receive acknowledgement
/// ## QoS2
/// ### Server-sent publish
/// 1. Recieve message
/// 2. Send Received message, transition to Received
/// 3. Receive Release message
/// 4. Send Complete message.
/// ### Client-sent publish
/// 1. Send publish, start at Sent
/// 2. Receive Received message
/// 3. Send Release message, transition to Released.
/// 4. Receive Complete message
pub enum PublishState<P> where P: Persistence {
    Sent(P::Key, Option<Sender<Result<ClientReturn>>>),
    Received(MqttPacket),
    Released(P::Key, Option<Sender<Result<ClientReturn>>>)
}

lazy_static!{
    static ref INVALID_MULTILEVEL: Regex = Regex::new("(?:[^/]#|#(?:.+))").unwrap();
    static ref INVALID_SINGLELEVEL: Regex = Regex::new(r"(?:[^/]\x2B|\x2B[^/])").unwrap();
}

pub struct TopicFilter {
    matcher: Regex,
    original: String
}

impl TopicFilter {
    pub fn from_string(s: &str) -> Result<TopicFilter> {
        // See if topic is legal
        if INVALID_SINGLELEVEL.is_match(s) || INVALID_MULTILEVEL.is_match(s) {
            bail!(ErrorKind::InvalidTopicFilter);
        }

        if s.is_empty() {
            bail!(ErrorKind::InvalidTopicFilter);
        }

        let mut collect: Vec<String> = Vec::new();
        for tok in s.split("/") {
            if tok.contains("+") {
                collect.push(String::from("[^/]+"));
            } else if tok.contains("#") {
                collect.push(String::from("?.*"));
            } else {
                collect.push(escape(tok))
            }
        }
        let reg = format!("^{}$", collect.join("/"));
        Ok(TopicFilter {
            original: String::from(s),
            matcher: Regex::new(&reg).chain_err(|| ErrorKind::InvalidTopicFilter)?
        })
    }

    pub fn match_topic(&self, topic: &str) -> bool {
        self.matcher.is_match(topic)
    }
}

// TODO: More filter tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_filter() {
        let topic = "this/is/a/filter";
        let filter = TopicFilter::from_string(topic).unwrap();
        assert!(filter.match_topic(topic));
        assert!(!filter.match_topic("this/is/wrong"));
        assert!(!filter.match_topic("/this/is/a/filter"));
    }

    #[test]
    fn single_level_filter() {
        let filter_str = "this/is/+/level";
        let filter = TopicFilter::from_string(filter_str).unwrap();
        assert!(filter.match_topic("this/is/single/level"));
        assert!(!filter.match_topic("this/is/not/valid/level"));
    }

    #[test]
    fn complex_single_level_filter() {
        let filter_str = "+/multi/+/+";
        let filter = TopicFilter::from_string(filter_str).unwrap();
        assert!(filter.match_topic("anything/multi/foo/bar"));
        assert!(!filter.match_topic("not/multi/valid"));
    }
}
