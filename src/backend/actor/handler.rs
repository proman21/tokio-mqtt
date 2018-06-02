/// This trait is implemented by actors to provide a handler for a particular message. It also
/// defines the response and error type for the message. Importantly, this response can either be
/// immediate or lazy; either the actor has an answer available during this message's processing, or
/// it needs to wait for future messages to provide an answer.
pub trait Handler<M> {
    type Item;
    type Error;

    fn handle<R>(&mut self, msg: M, &mut ctx: Context<Self>) -> R
        where R: Into<Response<Self, Self::Item, Self::Error>>;
}

pub enum Response<A, T, E> where A: Actor {
    Result(Result<T, E>),
    Future(Box<Future<Item=T, Error=E>>)
}

impl<A> From<Result<T, E>> for Response<A, T, E> where A: Actor{
    fn from(t: Result<T, E>) -> Response<A, T, E> {
        Response::Result(t)
    }
}

impl<A> From<F> for Response<A, T, E>
    where A: Actor,
          F: Future<Item=T, Error=E> {

    fn from(t: F) -> Response<A, T, E> {
        Response::Future(Box::new(t))
    }
}