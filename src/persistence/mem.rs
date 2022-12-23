use std::collections::HashMap;
use std::convert::Infallible;

use mqtt3_proto::{MqttPacket, Error};
use crate::persistence::Persistence;

/// In-memory persistence storage. Does violate the requirement for non-volatile persistence, but can be useful when
/// persistence is not really necessary.
pub struct MemoryPersistence {
    map: HashMap<String, Vec<u8>>
}

impl<'a> Persistence<'a> for MemoryPersistence {
    type Error = Error<'a>;

    fn put(&'a mut self, key: &str, packet: &MqttPacket<'a>) -> Result<(), Self::Error> {
        let mut buf = Vec::with_capacity(packet.len()?);
        packet.encode(&mut buf)?;
        self.map.insert(String::from(key), buf);
        Ok(())
    }

    fn get(&'a mut self, key: &str) -> Result<Option<MqttPacket<'a>>, Self::Error> {
        self.map.get(key)
            .map(|s| MqttPacket::from_buf(s)
                .map(|p| p.expect("Memory persistence curruption").1))
            .transpose()
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