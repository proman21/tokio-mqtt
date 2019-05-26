//! This is the actor cell. It holds the mailbox, actor, and other information that is used to drive
//! the actor's behaviour. It operates like a future, and is spawned onto an executor.

struct ActorCell<A>
