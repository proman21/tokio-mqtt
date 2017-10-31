use ::bytes::Bytes;
use ::errors::Error;
use ::futures::future::Future;
use ::futures::sink::BoxSink;
use ::futures::stream::Stream;

pub type BoxMqttFuture<T> = Box<Future<Item=T, Error=Error>>;
pub type BoxMqttSink<T> = BoxSink<T, Error>;
pub type BoxMqttStream<T> = Box<Stream<Item=T, Error=Error>>;

pub type SubItem = (String, Bytes);
pub type SubscriptionStream = BoxMqttStream<SubItem>;
