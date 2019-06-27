use std::error;
use futures::Future;

mod mem;

pub use self::mem::MemoryPersistence;

/// This trait provides an interface for basic session persistence functionality required by the MQTT client.
/// This cache should be persisted to a non-volatile storage medium so that a device shutdown does
/// not cause the client to lose QoS1/QoS2 messages that were in-flight.
///
/// The following specification should be observed by any implementation of this trait. "It"
/// refers to the implementation.
///  1. It **MUST** provide fast random access to a key-value pair. This can be achieved with an
///     in-memory cache of key-value pairs.
///  2. It's backing store **SHOULD** be persisted to a non-volatile storage medium.
///  3. It **MUST** atomically process the `insert` operation. Either the packet is successfully
///     processed and saved, or it fails.
///  4. It **MAY** batch `remove` operations, but it **MUST** appear to have happened instantly
///     (the value referred to by the key must not be retrievable after deletion).
///  5. It **MAY** delay the processing of the `clear` operation, but **MUST** appeared to have
///     happened instantly.
///
pub trait Persistence {
    type Error: error::Error + Send;
    /// Put a packet into the store using the specified key.
    fn put(&mut self, key: String, packet: &[u8]) -> Box<dyn Future<Item=(), Error=Self::Error>>;
    /// Retrieve a packet from the store, if it exists.
    fn get(&mut self, key: &str) -> Box<dyn Future<Item=Option<&[u8]>, Error=Self::Error>>;
    /// Remove a packet from the store, if it exists.
    fn remove(&mut self, key: &str) -> Box<dyn Future<Item=(), Error=Self::Error>>;
    /// Return a Vec of keys in the store.
    fn keys(&mut self) -> Box<dyn Future<Item=Vec<String>, Error=Self::Error>>;
    /// Clear the store of packets.
    fn clear(&mut self) -> Box<dyn Future<Item=(), Error=Self::Error>>;
}
