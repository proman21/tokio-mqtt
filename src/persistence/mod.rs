mod fs;

pub use self::fs::FsPersistence;

use std::error;
/// This trait provides an interface for basic cache functionality required by the MQTT client.
/// This cache should be persisted to a non-volatile storage medium so that a device shutdown does
/// not cause the client to lose QoS1/QoS2 messages that were in-flight. This object is used over
/// the lifetime of a client, meaning it may be used for multiple sessions with different servers
/// and client ID's.
///
/// The following specification should be observed by any implementation of this trait. "It"
/// refers to the implementation.
///  1. It **MUST** provide fast random access to a key-value pair. This can be achieved with an
///     in-memory cache of key-value pairs.
///  2. It's backing store **SHOULD** be persisted to a non-volatile storage medium.
///  3. It **MUST** atomically process the `insert` operation. Either the packet is successfully
///     processed and saved to a non-volatile storage medium, or it fails.
///  4. It **MAY** batch `remove` operations, but it **MUST** appear to have happened instantly
///     (the value referred to by the key must not be retrievable after deletion).
///  5. It **MAY** delay the processing of the `clear` operation, but **MUST** appeared to have
///     happened instantly.
///
pub trait Persistence {
    type Error: error::Error + Send + 'static;
    /// Open a persistence store for the specified client ID and server URI
    fn open(&mut self, client_id: String, server_uri: String) -> Result<(), Self::Error>;
    /// Close the currently opened store
    fn close(&mut self) -> Result<(), Self::Error>;
    /// Put a packet into the cache using the specified key.
    fn put(&mut self, key: String, packet: Vec<u8>) -> Result<(), Self::Error>;
    /// Retrieve a packet from the cache
    fn get(&mut self, key: &str) -> Result<Vec<u8>, Self::Error>;
    /// Return whether or not a key exists in the store.
    fn contains(&mut self, key: &str) -> Result<Option<()>, Self::Error>;
    /// Remove a packet from the cache. Returns nothing if successful.
    fn remove(&mut self, key: &str) -> Result<(), Self::Error>;
    /// Return a Vec of keys in the cache.
    fn keys(&mut self) -> Result<Vec<String>, Self::Error>;
    /// Clear the cache of packets.
    fn clear(&mut self) -> Result<(), Self::Error>;
}
