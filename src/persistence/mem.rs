use std::collections::HashMap;
use std::convert::Infallible;

use crate::persistence::Persistence;

/// In-memory persistence storage. Does violate the requirement for non-volatile persistence, but can be useful when
/// persistence is not really necessary.
pub struct MemoryPersistence {
    map: HashMap<String, Vec<u8>>
}

impl<'a> Persistence<'a> for MemoryPersistence {
    type Error = Infallible;

    fn put(&'a mut self, key: String, packet: &[u8]) -> Result<(), Self::Error> {
        self.map.insert(key, Vec::from(packet));
        Ok(())
    }

    fn get(&'a mut self, key: &str) -> Result<Option<&[u8]>, Self::Error> {
        Ok(self.map.get(key).map(|s| s.as_ref()))
    }

    fn remove(&'a mut self, key: &str) -> Result<(), Self::Error> {
        self.map.remove(key);
        Ok(())
    }

    fn keys(&'a mut self) -> Result<Vec<String>, Self::Error> {
        Ok(self.map.keys().cloned().collect())
    }

    fn clear(&'a mut self) -> Result<(), Self::Error> {
        self.map.clear();
        Ok(())
    }
}