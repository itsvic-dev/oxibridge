use std::error::Error;

use minicord::{Client, Gateway, GatewayMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let Ok(token) = std::env::var("TOKEN") else {
        println!("Please provide a token via the `TOKEN` environment variable.");
        return Ok(());
    };

    let client = Client::new(&token)?;

    let (tx, mut rx) = tokio::sync::broadcast::channel(32);
    let gw_task = tokio::spawn(async move {
        assert!(
            client.start_gateway(tx).await.is_ok(),
            "Failed to start gateway"
        );
    });

    let event = rx.recv().await?;
    if let GatewayMessage::Ready(data) = event {
        println!(
            "signed in as {}",
            data.user.global_name.unwrap_or(data.user.username)
        );
    }

    gw_task.await?;

    Ok(())
}
