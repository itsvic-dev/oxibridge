use crate::Client;

#[async_trait::async_trait]
pub trait Gateway {
    async fn start_gateway(&self);
}

#[async_trait::async_trait]
impl Gateway for Client {
    async fn start_gateway(&self) {
        todo!()
    }
}
