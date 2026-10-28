# kove

A client for the [Kove](https://kove.social) REST API and gateway, written for bots and bridges.

```rust,no_run
use kove::{Client, Gateway, Intent};

# async fn run() -> Result<(), kove::Error> {
let client = Client::new("bot_…");
let mut gateway = Gateway::new(&client, vec![Intent::Messages]);
while let Ok(dispatch) = gateway.next().await {
    println!("{:?}", dispatch.event);
}
# Ok(())
# }
```

Set another server with `Client::with_base_url`, for example `https://staging.kove.social`.
