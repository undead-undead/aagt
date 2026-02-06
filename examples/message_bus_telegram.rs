// Example: Using Message Bus + Telegram Notifier

use aagt_core::bus::{MessageBus, InboundMessage, OutboundMessage};
use aagt_core::infra::TelegramNotifier;
use aagt_core::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Create Message Bus
    let bus = Arc::new(MessageBus::new(100));
    
    // 2. Create Telegram Notifier
    let telegram = TelegramNotifier::new(
        std::env::var("TELEGRAM_BOT_TOKEN")?,
        std::env::var("TELEGRAM_CHAT_ID")?
    );
    
    // 3. Simulate message flow
    
    // CLI sends message to Agent
    let cli_msg = InboundMessage::new("cli", "user", "direct", "What's Bitcoin price?");
    bus.publish_inbound(cli_msg).await?;
    
    // Agent consumes message
    let msg = bus.consume_inbound().await?;
    println!("Agent received: {}", msg.content);
    
    // Agent processes and responds
    let response = OutboundMessage::new("cli", "direct", "BTC is $43,200");
    bus.publish_outbound(response.clone()).await?;
    
    // Agent also sends notification to Telegram
    telegram.notify(&format!("📊 {}", response.content)).await?;
    println!("✅ Sent notification to Telegram");
    
    // CLI consumes response
    let reply = bus.consume_outbound().await?;
    println!("User received: {}", reply.content);
    
    Ok(())
}

/*
Expected Output:

Agent received: What's Bitcoin price?
✅ Sent notification to Telegram
User received: BTC is $43,200

On Telegram:
📊 BTC is $43,200
*/
