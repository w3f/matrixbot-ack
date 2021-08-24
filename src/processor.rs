use actix::prelude::*;

pub struct Processor {

}

impl Default for Processor {
	fn default() -> Self {
		panic!("Processor was not initialized");
	}
}

impl Actor for Processor {
    type Context = Context<Self>;
}

impl SystemService for Processor {}
impl Supervised for Processor {}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub enum UserAction {}

#[derive(Clone, Debug, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct InsertAlert {

}

impl Handler<UserAction> for Processor {
	type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: UserAction, _ctx: &mut Self::Context) -> Self::Result {
		unimplemented!()
	}
}

impl Handler<InsertAlert> for Processor {
	type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: InsertAlert, _ctx: &mut Self::Context) -> Self::Result {
		unimplemented!()
	}
}
