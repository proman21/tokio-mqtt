use ::persistence::Persistence;
use ::linked_hash_map::LinkedHashMap;
use std::error;
use std::fmt;

/// Error in the MemoryPersistence, which is not possible.
#[derive(Debug)]
pub struct MemoryError;

impl error::Error for MemoryError {
    fn description(&self) -> &str {
        return "Error in the memory persistence"
    }
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "MemoryError")
    }
}

pub struct MemoryPersistence {
    store: LinkedHashMap<usize, Vec<u8>>,
    count: usize
}

impl MemoryPersistence {
    pub fn new() -> MemoryPersistence {
        MemoryPersistence {
            store: LinkedHashMap::new(),
            count: 0
        }
    }
}

impl Persistence for MemoryPersistence {
    type Key = usize;
    type Error = MemoryError;

    fn append(&mut self, packet: Vec<u8>) -> Result<Self::Key, Self::Error> {
        self.count += 1;
        self.store.insert(self.count, packet);
        Ok(self.count)
    }

    fn get(&mut self, key: Self::Key) -> Result<Vec<u8>, Self::Error> {
        if let Some(p) = self.store.get(&key) {
            Ok(p.clone())
        } else {
            unreachable!()
        }
    }

    fn remove(&mut self, key: Self::Key) -> Result<(), Self::Error> {
        self.store.remove(&key);
        Ok(())
    }

    fn keys(&mut self) -> Result<Vec<Self::Key>, Self::Error> {
        Ok(self.store.keys().map(|v| *v).collect())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.store.clear();
        Ok(())
    }
}
