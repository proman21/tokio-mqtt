use std::pin::Pin;
use snafu::ResultExt;

use tokio::prelude::*;
use tokio::stream::Stream;
use futures::task::*;
use tokio::sync::mpsc::{unbounded_channel as unbounded, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot::{channel, Receiver, Sender};

use crate::backend::ReactorCommand;
use crate::errors::*;

/// The sending end of an MPSC unbounded channel used to communicate with the MQTT Loop.
#[derive(Clone)]
pub struct LoopSender<'a>(UnboundedSender<ReactorCommand<'a>>);

impl<'a> LoopSender<'a> {
    fn pin_sender(self: Pin<&mut Self>) -> Pin<&mut UnboundedSender<ReactorCommand<'a>>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }
    }
}

impl<'a> Sink<ReactorCommand<'a>> for LoopSender<'a> {
    type Error = Error<'a>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_ready(cx).map(|res| res.context(LoopCommsError))
    }

    fn start_send(self: Pin<&mut Self>, msg: ReactorCommand<'a>) -> MqttResult<'a, ()> {
        self.pin_sender().start_send(msg).context(LoopCommsError)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_flush(cx).map(|res| res.context(LoopCommsError))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_close(cx).map(|res| res.context(LoopCommsError))
    }
}

/// The receiving end of an MPSC unbounded channel used to communicate with the MQTT Loop.
pub struct LoopReceiver<'a>(UnboundedReceiver<ReactorCommand<'a>>);

impl<'a> LoopReceiver<'a> {
    pub fn close(&mut self) {
        self.0.close()
    }

    fn pin_receiver(self: Pin<&mut Self>) -> Pin<&mut UnboundedReceiver<ReactorCommand<'a>>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }
    }
}

impl<'a> Stream for LoopReceiver<'a> {
    type Item = ReactorCommand<'a>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.pin_receiver().poll_next(cx)
    }
}

/// Create a loop channel sender, receiver pair.
pub fn loop_channel<'a>() -> (LoopSender<'a>, LoopReceiver<'a>) {
    let (send, recv) = unbounded();
    (LoopSender(send), LoopReceiver(recv))
}

pub struct LoopRetSender<'a, T>(Sender<MqttResult<'a, T>>);

impl<'a, T> LoopRetSender<'a, T> {
    pub fn send(self, t: MqttResult<'a, T>) -> MqttResult<'a, ()> {
        self.0.send(t).map_err(|request| Error::HandlerUnavailable)
    }

    pub fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
}

pub struct LoopRetReceiver<'a, T>(Receiver<MqttResult<'a, T>>);

impl<'a, T> LoopRetReceiver<'a, T> {
    pub fn close(&mut self) {
        self.0.close()
    }

    fn pin_receiver(self: Pin<&mut Self>) -> Pin<&mut Receiver<MqttResult<'a, T>>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }
    }
}

impl<'a, T> Future for LoopRetReceiver<'a, T> {
    type Output = MqttResult<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.pin_receiver().poll(cx) {
            Poll::Ready(res) => Poll::Ready(res.context(LoopUnavailable)?),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub fn loop_return<'a, T>() -> (LoopRetSender<'a, T>, LoopRetReceiver<'a, T>) {
    let (send, recv) = channel::<MqttResult<'a, T>>();
    (LoopRetSender(send), LoopRetReceiver(recv))
}

#[derive(Clone)]
pub struct SubscribeSink<'a>(UnboundedSender<(&'a str, &'a [u8])>);

impl<'a> SubscribeSink<'a> {
    fn pin_sender(self: Pin<&mut Self>) -> Pin<&mut UnboundedSender<(&'a str, &'a [u8])>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }
    }
}

impl<'a> Sink<(&'a str, &'a [u8])> for SubscribeSink<'a> {
    type Error = Error<'a>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_ready(cx).map(|res| res.context(LoopCommsError))
    }

    fn start_send(self: Pin<&mut Self>, msg: (&'a str, &'a [u8])) -> MqttResult<'a, ()> {
        self.pin_sender().start_send(msg).context(LoopCommsError)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_flush(cx).map(|res| res.context(LoopCommsError))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<MqttResult<'a, ()>> {
        self.pin_sender().poll_close(cx).map(|res| res.context(LoopCommsError))
    }
}

pub struct Subscription<'a>(&'a str, UnboundedReceiver<(&'a str, &'a [u8])>);

impl<'a> Subscription<'a> {
    fn pin_receiver(self: Pin<&mut Self>) -> Pin<&mut UnboundedReceiver<(&'a str, &'a [u8])>> {
        unsafe { self.map_unchecked_mut(|s| &mut s.1) }
    }
}

impl<'a> Stream for Subscription<'a> {
    type Item = (&'a str, &'a [u8]);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.pin_receiver().poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.1.size_hint()
    }
}

pub fn subscription_pair<'a>(topic: &'a str) -> (SubscribeSink<'a>, Subscription<'a>) {
    let (send, recv) = unbounded();
    (SubscribeSink(send), Subscription(topic, recv))
}