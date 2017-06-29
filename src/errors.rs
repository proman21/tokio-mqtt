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

        PacketDecodingError {
            description("Error decoding packet")
        }

        PacketEncodingError {
            description("Error encoding packet")
        }

        LoopCommsError {
            description("An error occured communicating with the loop")
        }

        LoopAbortError {
            description("Loop unexpectedly aborted")
        }

        LoopError {
            description("An error occurred while processing another request")
        }

        UnexpectedDisconnect {
            description("Network connection to server ended unexpectedly")
        }

        InvalidTopicFilter {
            description("Topic filter has invalid syntax")
        }

        AlreadyConnected {
            description("Client already connected to server")
        }

        ClientUnavailable {
            description("Client is stopped or is disconnecting")
        }
    }
}

impl Error {
    pub fn is_transient(&self) -> bool {
        match *self {
            Error(ErrorKind::StringConversionError, _) |
            Error(ErrorKind::InvalidTopicFilter, _) |
            Error(ErrorKind::AlreadyConnected, _) => true,
            _ => false
        }
    }
}

impl From<self::proto::ErrorKind> for Error {
    fn from(e: self::proto::ErrorKind) -> Error {
        Error::from(ErrorKind::Protocol(e))
    }
}

pub mod proto {
    use ::proto::{PacketType, ConnRetCode, QualityOfService};
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

            ConnectionRefused(c: ConnRetCode) {
                description("Connect request denied by the server")
                display("Server denied connection with this response: {}", c)
            }

            SubscriptionRejected(t: String, q: QualityOfService) {
                description("Server rejected a subscription")
                display("A subscription to '{}'@{} was rejected by the server", t, q)
            }

            InvalidPacket(s: String) {
                description("An invalid packet was encountered")
                display("Invalid packet found while decoding: {}", s)
            }

            QualityOfServiceError(q: QualityOfService, s: String) {
                description("An error occurred while processing a service flow")
                display("While processing a {} flow, the following error occurred: {}", q, s)
            }
        }
    }
}
