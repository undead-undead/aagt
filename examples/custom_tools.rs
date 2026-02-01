/// Example demonstrating custom tool creation and registration
/// 
/// This example shows:
/// - Creating custom tools using the simple_tool! macro
/// - Registering tools with an agent
/// - Agent automatically calling tools based on user input

use aagt_core::{prelude::*, simple_tool};
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};
use anyhow::Result;
use serde_json::json;

// Define a simple weather tool
simple_tool!(
    GetWeather,
    "get_weather",
    "Get current weather information for a city",
    {
        city: ("string", "The name of the city")
    },
    [city],
    |args| async move {
        let city = args["city"].as_str().unwrap();
        // In a real implementation, this would call a weather API
        Ok(format!("Weather in {}: Sunny, 25°C, Light breeze", city))
    }
);

// Define a calculator tool
simple_tool!(
    Calculate,
    "calculate",
    "Perform basic arithmetic calculations",
    {
        expression: ("string", "Mathematical expression (e.g., '2 + 2')")
    },
    [expression],
    |args| async move {
        let expr = args["expression"].as_str().unwrap();
        // Simplified calculation (in production, use a proper parser)
        let result = match expr {
            "2 + 2" => "4",
            "10 * 5" => "50",
            _ => "Unable to calculate",
        };
        Ok(format!("Result: {}", result))
    }
);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let provider = Gemini::from_env()?;

    // Build agent with custom tools
    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble("You are a helpful assistant with access to weather and calculator tools.")
        .tool(Box::new(GetWeather))
        .tool(Box::new(Calculate))
        .build()?;

    // Test tool usage
    println!("=== Testing Weather Tool ===");
    let response = agent.prompt("What's the weather like in Tokyo?").await?;
    println!("Agent: {}\n", response);

    println!("=== Testing Calculator Tool ===");
    let response = agent.prompt("What is 2 + 2?").await?;
    println!("Agent: {}\n", response);

    Ok(())
}
