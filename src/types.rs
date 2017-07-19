use ::bytes::Bytes;
use ::errors::Error;
use ::futures::future::BoxFuture;
use ::futures::sink::BoxSink;
use ::futures::stream::BoxStream;

pub type BoxMqttFuture<T> = BoxFuture<T, Error>;
pub type BoxMqttSink<T> = BoxSink<T, Error>;
pub type BoxMqttStream<T> = BoxStream<T, Error>;

pub type SubItem = (String, Bytes);
pub type SubscriptionStream = BoxMqttStream<SubItem>;
