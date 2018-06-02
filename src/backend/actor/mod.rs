//! A custom actor impl designed specifically for this library.
//!
//! This library assumes that it is being scheduled on a tokio `Reactor`, so it's semantics have
//! been tuned to account for this. It also only support single message types, so if different
//! messages are required you should use a enum. Finally, it bakes in certain mailbox semantics,
//! scheduling decisions, and other actor model ideas that I need for this library.