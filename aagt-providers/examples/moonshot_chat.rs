use aagt_core::message::Message;
use aagt_core::provider::Provider;
use aagt_providers::moonshot::{Moonshot, MOONSHOT_V1_8K};
use futures::StreamExt;
use std::io::Write;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // 1. Initialize Provider
    // Requires MOONSHOT_API_KEY env var
    let provider = match Moonshot::from_env() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Please set MOONSHOT_API_KEY environment variable to run this example.");
            return Ok(());
        }
    };

    println!("🤖 Using Provider: {}", provider.name());

    // 2. Create Conversation
    let messages = vec![
        Message::system("You are a helpful assistant involved in high-frequency trading."),
        Message::user("Hello Kimi! Can you explain what 'slippage' is in 10 words?"),
    ];

    // 3. Stream Response
    println!("\nUser: {}", messages.last().unwrap().text());
    print!("Assistant: ");
    std::io::stdout().flush()?;

    let mut stream = provider
        .stream_completion(
            MOONSHOT_V1_8K,
            None,
            messages,
            vec![],
            Some(0.7),
            None,
            None,
        )
        .await?;

    let mut full_response = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(choice) => {
                if let aagt_core::streaming::StreamingChoice::Message(text) = choice {
                    print!("{}", text);
                    std::io::stdout().flush()?;
                    full_response.push_str(&text);
                }
            }
            Err(e) => eprintln!("\nError: {}", e),
        }
    }
    println!("\n");

    Ok(())
}
