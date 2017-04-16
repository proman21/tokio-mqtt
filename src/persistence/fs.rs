use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fs::{read_dir, remove_file, File};
use std::io::{self, Read, Write};
use ::persistence::Persistence;
use ::touch::{dir, file};

pub struct FSPersistence {
    dir: PathBuf,
    index: u64,
    cache: BTreeMap<u64, Vec<u8>>
}

impl FSPersistence {
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<FSPersistence> {
        let p = path.as_ref();
        dir::create(p.to_str().ok_or(io::Error::new(io::ErrorKind::Other, "Error ceating directory"))?).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(FSPersistence {
            dir: p.to_path_buf(),
            index: 0,
            cache: BTreeMap::new()
        })
    }
}

impl Persistence for FSPersistence {
    type Key = u64;
    type Error = io::Error;

    fn append(&mut self, packet: Vec<u8>) -> Result<Self::Key, Self::Error> {
        let key = self.index;
        self.index += 1;
        // Check if this packet already exists using the cache as a source of truth.
        match self.cache.entry(key) {
            Entry::Occupied(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Entry::Vacant(v) => {
                // Update cache
                let p = v.insert(packet);
                // Persist to the filesystem.
                let path = self.dir.with_file_name(format!("{:x}", key));
                let mut file = File::create(path)?;
                file.write_all(&p)?;
                Ok(key)
            }
        }
    }

    fn get(&mut self, key: Self::Key) -> Result<Vec<u8>, Self::Error> {
        // Check cache, then check filesystem
        match self.cache.entry(key) {
            Entry::Occupied(o) => Ok(o.get().clone()),
            Entry::Vacant(v) => {
                let path = self.dir.with_file_name(format!("{:x}", key));
                let mut file = File::open(path)?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                let p = v.insert(buf);
                Ok(p.clone())
            }
        }
    }

    fn remove(&mut self, key: Self::Key) -> Result<(), Self::Error> {
        // Remove from cache and filesystem
        match self.cache.entry(key) {
            Entry::Vacant(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Entry::Occupied(o) => {
                let (key, _) = o.remove_entry();
                let path = self.dir.with_file_name(format!("{:x}", key));
                let path_s = path.to_str().ok_or(io::Error::from(io::ErrorKind::InvalidInput))?;
                file::delete(path_s).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }
        }
    }

    fn keys(&mut self) -> Result<Vec<Self::Key>, Self::Error> {
        if self.cache.is_empty() {
            // Populate this instance
            let mut collect = Vec::new();
            for entry in read_dir(self.dir.to_path_buf())? {
                let e = entry?;
                let path = e.path();
                if path.is_file() {
                    collect.push(path.file_stem().unwrap().to_os_string().into_string().unwrap());
                } else {
                    continue;
                }
            }
            &collect.sort();
            for filename in collect {
                let key = u64::from_str_radix(&filename, 16)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let path = self.dir.with_file_name(filename);
                let mut file = File::open(path)?;
                let mut v = Vec::new();
                file.read_to_end(&mut v)?;
                self.cache.insert(key, v);
                self.index = key;
            }
            self.index += 1;
        }

        Ok(self.cache.keys().cloned().collect())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.cache.clear();
        for entry in read_dir(self.dir.to_path_buf())? {
            let e = entry?;
            let path = e.path();
            if path.is_file() {
                remove_file(path)?
            } else {
                continue;
            }
        }
        Ok(())
    }
}
