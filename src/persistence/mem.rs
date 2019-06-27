use std::collections::HashMap;
use std::convert::Infallible;

use futures::prelude::*;
use futures::future::{ok};
use persistence::Persistence;

/// In-memory persistence storage. Does violate the requirement for non-volatile persistence, but can be useful when
/// persistence is not really necessary.
pub struct MemoryPersistence {
    map: HashMap<String, Vec<u8>>
}

impl Persistence for MemoryPersistence {
    type Error = Infallible;

    fn put(&mut self, key: String, packet: &[u8]) -> Box<dyn Future<Item=(), Error=Self::Error>> {
        self.map.insert(key, Vec::from(packet));
        ok(())
    }

    fn get(&mut self, key: &str) -> Box<dyn Future<Item=Option<&[u8]>, Error=Self::Error>> {
        ok(self.map.get(key))
    }

    fn remove(&mut self, key: &str) -> Box<dyn Future<Item=(), Error=Self::Error>> {
        self.map.remove(key);
        ok(())
    }

    fn keys(&mut self) -> Box<dyn Future<Item=Vec<String>, Error=Self::Error>> {
        ok(self.map.keys().cloned().collect())
    }

    fn clear(&mut self) -> Box<dyn Future<Item=(), Error=Self::Error>> {
        self.map.clear();
        ok(())
    }
}