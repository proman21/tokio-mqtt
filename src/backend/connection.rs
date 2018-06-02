use futures::unsync::mpsc::{UnboundedSender, UnboundedReceiver};

type MqttFramedSink<I> = SplitSink<MqttFramed<I>>;
type MqttFramedStream<I> = SplitStream<MqttFramed<I>>;

pub enum Message {
    Send(MqttPacket),
    Close
}

#[derive(StateMachineFuture)]
pub enum Connection<I> where I: AsyncRead + AsyncWrite {
    #[state_machine_future(start, transitions(Sending, Close, Ready, Error))]
    Polling {
        receive: UnboundedReceiver<Message>,
        lp: LoopMpscSender<ClientMesage>,
        write: MqttFramedSink<I>,
        read: MqttFramedStream<I>
    },
    #[state_machine_future(transitions(Polling, Ready, Error))]
    Sending {
        receive: UnboundedReceiver<Message>,
        lp: LoopMpscSender<ClientMesage>,
        sending: Send<MqttFramedSink<I>>,
        read: MqttFramedStream<I>
    },
    #[state_machine_future(transitions(Ready, Error))]
    Close {
        lp: LoopMpscSender<ClientMessage>,
        write: MqttFramedSink<I>
    },
    #[state_machine_future(ready)]
    Ready(()),
    #[state_machine_future(error)]
    Error(())
}

impl<I> PollConnection for Connection<I> {
    fn poll_polling<'a>(polling: &'a mut RentToOwn<'a, Polling>) -> Poll<AfterPolling, Error> {
        // See if any incoming packets are being received
        match polling.read.poll() {
            Ok(Async::Ready(Some(p))) => {
                match polling.lp.send(ClientMessage::Connection) {
                    Ok(_) => {},
                    Err(_) => Err(())
                }
            }
        }
    }
}

