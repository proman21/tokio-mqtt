use errors::{Error, ErrorKind, Result};
use futures::prelude::*;
use futures::sync::oneshot::Receiver;
use futures::sync::mpsc::UnboundedReceiver;
use proto::{MqttString, QualityOfService};

pub struct MqttFuture<T>(Receiver<Result<T>>);

impl<T> From<Receiver<Result<T>>> for MqttFuture<T> {
    fn from(value: Receiver<Result<T>>) -> Self {
        MqttFuture(value)
    }
}

impl<T> Future for MqttFuture<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        try_ready!(self.0.poll().map_err(|_| Error::from(ErrorKind::LoopCommsError)))
            .map(|t| t.into())
    }
}

pub struct MqttStream<T>(UnboundedReceiver<Result<T>>);

impl<T> From<UnboundedReceiver<Result<T>>> for MqttStream<T> {
    fn from(value: UnboundedReceiver<Result<T>>) -> Self {
        MqttStream(value)
    }
}

impl<T> Stream for MqttStream<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match try_ready!(self.0.poll().map_err(|_| Error::from(ErrorKind::LoopCommsError))) {
            Some(r) => r.map(|t| Some(t).into()),
            None => Ok(Async::Ready(None))
        }
    }
}

pub struct SubItem(pub(crate) MqttString, pub(crate) Vec<u8>);

impl SubItem {
    pub fn topic(&self) -> &String {
        &self.0
    }
    pub fn payload(&self) -> &Vec<u8> {
        &self.1
    }
}

pub struct Subscription {
    qos: QualityOfService,
    recv: MqttStream<SubItem>
}

impl Subscription {
    pub(crate) fn new(qos: QualityOfService, recv: MqttStream<SubItem>) -> Subscription {
        Subscription {
            qos,
            recv
        }
    }

    pub fn qos(&self) -> QualityOfService {
        self.qos.clone()
    }
}

impl Stream for Subscription {
    type Item = SubItem;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.recv.poll()
    }
}