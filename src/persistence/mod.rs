mod fs;

pub use self::fs::FSPersistence;

use std::error;
/// This trait provides an interface for basic cache functionality required by the MQTT client.
/// This cache should be presisted to a non-volatile storage medium so that a device shutdown does
/// not cause the client to lose QoS1 or QoS2 messages that were in-flight. The stored data is a
/// serialized packet structure.
///
/// The following specification should be observed by any implementation of this trait. "It"
/// refers to the implementation.
///  1. It **MUST** provide an ordered guarantee; the order of packet inserts should be retained.
///  2. It **MUST** provide fast random access to a key-value pair. This can be acheived with an
///     in-memory LRU cache of key-value pairs.
///  3. It's backing store **SHOULD** be persisted to a non-volatile storage medium.
///  4. It **MUST** atomically process the `insert` operation. Either the packet is successfully
///     processed and saved to a non-volatile storage medium, or it fails.
///  5. It **MAY** batch `remove` operations, but it **MUST** appear to have happened instantly
///     (the value referred to by the key must not be retrievable after deletion).
///  6. It **MAY** delay the processing of the `clear` operation, but **MUST** appeared to have
///     happened instantly.
///
/// Things to note.
///  - `keys` is called when the MQTT client first starts up, and the Clean Session flag is not
///    set to 0, so it can collect packets to retry sending. This is good time to populate any
///    caches with those packets.
///  - `clear` is called when the client requests a clean session, or the server rejects a session
///    requst from the client. Clear any backing stores and caches (if present)
///
pub trait Persistence {
    type Key: Send;
    type Error: error::Error + Send + 'static;
    /// Append a packet into the cache, returning a `Key` addressing the location if successful.
    fn append(&mut self, packet: Vec<u8>) -> Result<Self::Key, Self::Error>;
    /// Retrieve a packet from the cache
    fn get(&mut self, key: Self::Key) -> Result<Vec<u8>, Self::Error>;
    /// Remove a packet from the cache. Returns nothing if successful.
    fn remove(&mut self, key: Self::Key) -> Result<(), Self::Error>;
    /// Return a Vec of keys in the cache.
    fn keys(&mut self) -> Result<Vec<Self::Key>, Self::Error>;
    /// Clear the cache of packets.
    fn clear(&mut self) -> Result<(), Self::Error>;
}
