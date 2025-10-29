use std::{env, error::Error};

use minicord::{
    Client,
    types::{User, http::APIResponse},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let Ok(token) = env::var("TOKEN") else {
        println!("Please provide a token via the `TOKEN` environment variable.");
        return Ok(());
    };

    let client = Client::new(&token);
    let me_resp = client.get::<User>("users/@me").await?;

    let me = match me_resp {
        APIResponse::Success(me) => me,
        APIResponse::Error { message, .. } => {
            println!("Failed to get user: {message}");
            return Ok(());
        }
    };

    println!("Our user: {me:#?}");
    Ok(())
}
