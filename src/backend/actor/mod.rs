//! A custom actor impl designed specifically for this library.
//!
//! ## Caveats
//! - It only support single message types, so if different messages are required you should use a
//! enum.
//! - Bakes in certain mailbox semantics, scheduling decisions, and other actor model ideas that I
//! need for this library.
