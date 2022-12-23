#![allow(dead_code)]
#![allow(unused_variables)]
#![recursion_limit="128"]

extern crate tokio;
extern crate bytes;
#[macro_use] extern crate lazy_static;
extern crate bincode;
#[macro_use] extern crate slog;
extern crate slog_stdlog;
#[macro_use] extern crate derive_builder;
extern crate regex;
extern crate mqtt3_proto;
#[macro_use] extern crate snafu;
#[macro_use] extern crate futures;

mod errors;
mod client;
pub mod persistence;
mod backend;
mod topic_filter;
mod config;

pub use crate::errors::{MqttResult, Error};