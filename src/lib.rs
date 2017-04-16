#![allow(dead_code)]
#![allow(unused_variables)]

extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_tls;

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate nom;
#[cfg(test)]
#[macro_use]
extern crate nom_test_helpers;
#[macro_use]
extern crate enum_primitive;
extern crate bytes;
extern crate rustc_serialize;
extern crate touch;
extern crate linked_hash_map;
extern crate take;
extern crate regex;
#[macro_use]
extern crate lazy_static;

mod errors;
mod types;
mod proto;
mod client;
mod persistence;
mod tokio;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
