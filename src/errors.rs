use std;

error_chain! {
    links {
        Protocol(self::proto::Error, self::proto::ErrorKind);
    }

    foreign_links {
        Io(std::io::Error);
    }

    errors {
        PersistenceError {
            description("An error was encountered using a Persistence object")
        }

        StringConversionError {
            description("String too long to encode")
        }

        LoopCommsError {
            description("An error occured communicating with the loop")
        }

        LoopAbortError {
            description("Loop unexpectedly aborted")
        }

        UnexpectedDisconnect {
            description("Network connection to server ended unexpectedly")
        }

        InvalidTopicFilter {
            description("Topic filter has invalid syntax")
        }
    }
}

pub mod proto {
    use ::proto::{PacketType, ConnectReturnCode, QualityOfService};
    error_chain!{
        errors {
            ResponseTimeout(p: PacketType) {
                description("Timeout waiting for server response")
                display("Server took too long to respond to {:?} packet", p)
            }

            UnexpectedResponse(p: PacketType) {
                description("Client recived an unexpected response")
                display("Client recieved a unexpected {:?} packet from the server", p)
            }

            ConnectionRefused(c: ConnectReturnCode) {
                description("Connect request denied by the server")
                display("Server denied connection with this response: {}", c)
            }

            SubscriptionRejected(t: String, q: QualityOfService) {
                description("Server rejected a subscription")
                display("A subscription to '{}'@{} was rejected by the server", t, q)
            }
        }
    }
}
