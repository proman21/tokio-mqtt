#![allow(dead_code)]
#![allow(unused_variables)]
#![recursion_limit="128"]

#[macro_use]
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
extern crate touch;
extern crate linked_hash_map;
extern crate regex;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate bincode;
#[macro_use]
extern crate slog;
extern crate slog_stdlog;
#[macro_use]
extern crate derive_builder;


mod errors;
mod types;
mod proto;
mod client;
mod persistence;
mod backend;

