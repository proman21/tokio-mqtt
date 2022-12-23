use slog::{Logger, Drain};
use slog_stdlog;

use mqtt3_proto::types::{ProtoLvl, LWTMessage, Credentials};

/// Used to configure the client. Defaults are as follows
///  - Connect Timeout: 30 seconds
///  - Ping Timeout: 10 seconds
///  - Subscribe Batch Timeout: 0 seconds.
///  - Unsubscribe Batch Timeout: 0 seconds.
///  - Keep Alive: 0 (off)
///  - In-flight Window: 0 (off)
///  - Version: MQTT 3.1.1
///  - Credentials: None
///  - Last Will and Testament: None
///  - Clean Session: true
///  - Client ID: "tokio_mqtt"
///  - Logger: `log`
#[derive(Clone, Builder)]
#[builder(setter(into, strip_option))]
pub struct Config<'a> {
    /// Specify how long to wait to batch subscribe requests from the client.
    /// 0 means send subscribe packets immediately.
    #[builder(default = "0")]
    pub subscribe_batch_timeout: u64,
    /// Specify how long to wait to batch unsubscribe requests from the client.
    /// 0 means send unsubscribe packets immediately.
    #[builder(default = "0")]
    pub unsubscribe_batch_timeout: u64,
    /// Sets the keep alive value of the connection. If no response is received in `1.5k`
    /// seconds, the server will treat the client as disconnected. The client will send pings to
    /// the server every `k` seconds when idle.
    #[builder(default = "0")]
    pub keep_alive: u16,
    /// Specify which version of the MQTT protocol to use when communicating with the server.
    #[builder(default = "ProtoLvl::V3_1_1")]
    pub version: ProtoLvl,
    /// Sets the Last Will and Testament message. This is stored by the server and sent to the
    /// specified topic in the event that:
    ///  - The network connection fails
    ///  - The client fails to communicate before the keep-alive time expires
    ///  - The client closes the connection without sending a disconnect message
    ///  - A protocol error occurs
    #[builder(default)]
    pub lwt: Option<LWTMessage<'a, &'a [u8]>>,
    /// Specify the credentials the client will use when connecting to the server.
    #[builder(default)]
    pub credentials: Option<Credentials<'a, &'a [u8]>>,
    /// Specify whether the server should treat this session as clean.
    #[builder(default = "true")]
    pub clean: bool,
    /// Set the ID used to identify this client to the server. If clean is false and this client
    /// has a session stored on the server, you must set this value to the ID used in past sessions.
    #[builder(default = "self.generate_client_id()")]
    pub client_id: String,
    /// Specify a logger for the client. Defaults to passing to `log`
    #[builder(default = "self.default_logger()")]
    pub logger: Logger
}

impl<'a> ConfigBuilder<'a> {
    fn default_logger(&self) -> Logger {
        Logger::root(slog_stdlog::StdLog.fuse(), o!())
    }

    fn generate_client_id(&self) -> String {
        String::from("tokio_mqtt")
    }
}