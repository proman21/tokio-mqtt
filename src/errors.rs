use std::io;
use std::error;
use mqtt3_proto;
use crate::topic_filter;
use tokio::sync::mpsc::error::UnboundedSendError;
use tokio::sync::oneshot::error::RecvError;

pub type MqttResult<'a, T> = Result<T, Error<'a>>;

#[derive(Snafu, Debug)]
#[snafu(visibility(pub))]
pub enum Error<'a> {
    #[snafu(display("An error was encountered using a Persistence object: {}", source))]
    PersistenceError{ source: Box<dyn error::Error + 'a> },
    #[snafu(display("A MQTT Protocol Error occurred: {}", source))]
    ProtocolError{ source: mqtt3_proto::Error<'a> },
    #[snafu(display("Error occurred while compiling topic filter: {}", source))]
    TopicFilterError{ source: topic_filter::Error<'a> },
    #[snafu(display("I/O error occurred: {}", source))]
    IoError{ source: io::Error },
    #[snafu(display("An error occurred communicating with the loop: {}", source))]
    LoopCommsError{ source: UnboundedSendError },
    #[snafu(display("Loop has terminated while processing handler request: {}", source))]
    LoopUnavailable{ source: RecvError },
    #[snafu(display("Handler terminated before loop was able to complete request."))]
    HandlerUnavailable,
    #[snafu(display("Loop unexpectedly aborted."))]
    LoopAbortError,
    #[snafu(display("Loop encountered illegal state."))]
    LoopStateError,
    #[snafu(display("An error occurred while processing another request."))]
    LoopError,
    #[snafu(display("Network connection to server ended unexpectedly."))]
    UnexpectedDisconnect,
    #[snafu(display("Client already connected to server."))]
    AlreadyConnected,
    #[snafu(display("Client is stopped or is connecting/disconnecting."))]
    ClientUnavailable,
    #[snafu(display("Subscription to topic '{}' @ {} was rejected by the server.", topic, qos))]
    SubscriptionRejected{ topic: &'a String, qos: mqtt3_proto::QualityOfService },
}

impl<'a> From<io::Error> for Error<'a> {
    fn from(source: io::Error) -> Error<'a> {
        Error::IoError{ source }
    }
}

impl<'a> From<mqtt3_proto::Error<'a>> for Error<'a> {
    fn from(source: mqtt3_proto::Error<'a>) -> Error<'a> {
        Error::ProtocolError{ source }
    }
}
