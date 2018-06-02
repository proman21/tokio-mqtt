//! This is the actor cell. It holds the mailbox, actor, and other information that is used to drive
//! the actor's behaviour. It operates like a future, and is scheduled onto a tokio `Reactor` that
//! drives it.

struct ActorCell<A>