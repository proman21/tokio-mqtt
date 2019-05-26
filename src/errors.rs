use std;
use proto::MqttPacket;
use futures::sync::oneshot::Canceled;

error_chain! {
    links {
        Protocol(self::proto::Error, self::proto::ErrorKind);
    }

    foreign_links {
        Io(std::io::Error);
        LoopComms(Canceled);
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

        PacketEncodingError(p: MqttPacket) {
            description("Error encoding packet")
            display("A packet could not be encoded: {:?}", p)
        }

        LoopCommsError {
            description("An error occurred communicating with the loop")
        }

        LoopAbortError {
            description("Loop unexpectedly aborted")
        }

        LoopStateError {
            description("Loop encountered illegal state")
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
            description("Client is stopped or is connecting/disconnecting")
        }
    }
}
