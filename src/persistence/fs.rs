use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{read_dir, remove_file, File};
use std::io::{self, Read, Write};

use persistence::Persistence;
use touch::{dir, file, exists};

pub struct FsPersistence {
    root_dir: PathBuf,
    current_store: PathBuf,
    cache: HashMap<String, Vec<u8>>
}

impl FsPersistence {
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<FsPersistence> {
        let p = path.as_ref();
        dir::create(
            p.to_str().ok_or(io::Error::new(io::ErrorKind::Other, "Error creating directory"))?
        ).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(FsPersistence {
            root_dir: p.to_path_buf(),
            current_store: PathBuf::new(),
            cache: HashMap::new()
        })
    }
}

impl Persistence for FsPersistence {
    type Error = io::Error;

    fn open(&mut self, client_id: String, server_uri: String) -> Result<(), Self::Error> {
        self.current_store = self.root_dir.join(format!("{}@{}", client_id, server_uri));
        Ok(())
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        self.cache.clear();
        self.current_store = PathBuf::new();
        Ok(())
    }

    fn put(&mut self, key: String, packet: Vec<u8>) -> Result<(), Self::Error> {
        // Check if this packet already exists using the cache as a source of truth.
        match self.cache.entry(key) {
            Entry::Occupied(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Entry::Vacant(v) => {
                // Update cache
                let p = v.insert(packet);
                // Persist to the filesystem.
                let path = self.current_store.with_file_name(key);
                let mut file = File::create(path)?;
                file.write_all(&p)?;
                Ok(())
            }
        }
    }

    fn contains(&mut self, key: &str) -> Result<Option<()>, Self::Error> {
        match self.cache.entry(key.to_owned()) {
            Entry::Occupied(o) => Ok(Some(())),
            Entry::Vacant(v) => {
                let path = self.current_store.with_file_name(key);
                if path.exists() {
                    let mut file = File::open(path)?;
                    let mut buf = Vec::new();
                    file.read_to_end(&mut buf)?;
                    let p = v.insert(buf);
                    Ok(Some(()))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn get(&mut self, key: &str) -> Result<Vec<u8>, Self::Error> {
        // Check cache, then check filesystem
        match self.cache.entry(key.to_owned()) {
            Entry::Occupied(o) => Ok(o.get().clone()),
            Entry::Vacant(v) => {
                let path = self.current_store.with_file_name(key);
                let mut file = File::open(path)?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                let p = v.insert(buf);
                Ok(p.clone())
            }
        }
    }

    fn remove(&mut self, key: &str) -> Result<(), Self::Error> {
        // Remove from cache and filesystem
        match self.cache.entry(key.to_owned()) {
            Entry::Vacant(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Entry::Occupied(o) => {
                let (key, _) = o.remove_entry();
                let path = self.current_store.with_file_name(key);
                let path_s = path.to_str().ok_or(io::Error::from(io::ErrorKind::InvalidInput))?;
                file::delete(path_s).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }
        }
    }

    fn keys(&mut self) -> Result<Vec<String>, Self::Error> {
        if self.cache.is_empty() {
            // Populate this instance
            let mut collect = Vec::new();
            for entry in read_dir(self.current_store)? {
                let e = entry?;
                let path = e.path();
                if path.is_file() {
                    collect.push(path);
                }
            }
            for path in collect {
                let key = path.file_name().unwrap().to_owned().into_string().unwrap();
                let mut file = File::open(path)?;
                let mut v = Vec::new();
                file.read_to_end(&mut v)?;
                self.cache.insert(key, v);
            }
        }

        Ok(self.cache.keys().cloned().collect())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.cache.clear();
        for entry in read_dir(self.current_store)? {
            let e = entry?;
            let path = e.path();
            if path.is_file() {
                remove_file(path)?
            }
        }
        Ok(())
    }
}
