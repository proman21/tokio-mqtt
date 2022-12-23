use std::time::Duration;

use futures::Future;
use futures::channel::oneshot::Sender;
use tokio_io::{AsyncRead, AsyncWrite};
use mqtt3_proto::{MqttPacket, PacketType};

use crate::persistence::Persistence;
use crate::backend::Loop;
use crate::errors::Error;

enum State<'a> {
    Start,
    Send()
}

pub struct PingHandler<'a> {
    ret: Sender<Result<(), Error<'a>>>
}

