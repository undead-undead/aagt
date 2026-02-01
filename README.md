# AAGT (AI Agent Trade)

**A lightweight, modular, and high-performance framework for building AI Agents in Rust.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)

AAGT provides the core abstractions and utilities needed to build complex, multi-agent systems with minimal overhead. It is designed to be flexible, safe, and easy to extend.

---

## 🚀 Features

*   **Modular Architecture**: Core logic (`aagt-core`) is separated from provider implementations (`aagt-providers`).
*   **Provider Agnostic**: Support for multiple LLM backends (OpenAI, Anthropic, Gemini, DeepSeek, OpenRouter) via a unified `Provider` trait.
*   **Robust Memory System**:
    *   **Short-term Memory**: Efficient ring-buffer conversation history.
    *   **Long-term Memory**: Persistent vector-store ready memory with token-aware retrieval.
*   **Multi-Agent Coordination (Swarm)**:
    *   **Dynamic Workflows**: Define agent chains dynamically at runtime.
    *   **Role-Based Routing**: Delegate tasks to specialized agents (Researcher, Trader, Risk Analyst).

---

## 📦 Installation

Add AAGT to your `Cargo.toml`:

```toml
[dependencies]
aagt-core = { git = "https://github.com/undead-undead/aagt", package = "aagt-core" }
aagt-providers = { git = "https://github.com/undead-undead/aagt", package = "aagt-providers" }
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
```

---

## 🛠️ Quick Start

### 1. Create a Simple Agent

```rust
use aagt_core::prelude::*;
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize Provider
    let provider = Gemini::from_env()?;

    // 2. Build Agent
    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble("You are a helpful assistant.")
        .build()?;

    // 3. Chat
    let response = agent.prompt("Hello, who are you?").await?;
    println!("{}", response);
    
    Ok(())
}
```

### 2. Add Custom Tools (Web3 Example)

```rust
use aagt_core::simple_tool;
use serde_json::json;

simple_tool!(
    SwapToken,
    "swap_token",
    "Swap tokens on a DEX (e.g., Jupiter on Solana)",
    {
        from_token: ("string", "Token to swap from (e.g., SOL)"),
        to_token: ("string", "Token to swap to (e.g., USDC)"),
        amount: ("number", "Amount to swap")
    },
    [from_token, to_token, amount],
    |args| async move {
        let from = args["from_token"].as_str().unwrap();
        let to = args["to_token"].as_str().unwrap();
        let amount = args["amount"].as_f64().unwrap();
        
        // Call your Web3 swap logic here
        Ok(format!("Swapped {} {} to {} successfully", amount, from, to))
    }
);

// Register tool with agent
let agent = Agent::builder(provider)
    .tool(Box::new(SwapToken))
    .build()?;
```

### 3. Multi-Agent Swarm

```rust
use aagt_core::multi_agent::{Coordinator, AgentRole};

// Initialize Coordinator
let mut coordinator = Coordinator::new();

// Register Agents
coordinator.register(AgentRole::Researcher, researcher_agent);
coordinator.register(AgentRole::Assistant, writer_agent);

// Define Workflow: Research -> Write
let workflow = vec![AgentRole::Researcher, AgentRole::Assistant];

// Execute
let result = coordinator.orchestrate("Research Rust async trends", workflow).await?;
```

---

## 🔍 Observability & Tracing

AAGT includes built-in tracing for debugging and monitoring:

```rust
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

// Initialize tracing
let subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .finish();
tracing::subscriber::set_global_default(subscriber)?;

// All agent operations are automatically traced
let agent = Agent::builder(provider).build()?;
let response = agent.prompt("Hello").await?;
// Logs: "Agent received prompt", "Agent completed chat", etc.
```

**Features:**
- ✅ Automatic instrumentation of all agent operations
- ✅ Performance metrics (execution time, token usage)
- ✅ Structured logging (JSON format for production)
- ✅ Zero performance overhead when disabled

**Examples:**
- [`examples/tracing_agent.rs`](./examples/tracing_agent.rs) - Basic tracing setup
- [`examples/production_tracing.rs`](./examples/production_tracing.rs) - Production config with log rotation

---

## 📂 Project Structure

```
aagt/
├── aagt-core/          # Core interfaces (Agent, Provider, Tool, Memory, MultiAgent)
├── aagt-providers/     # LLM provider implementations (OpenAI, Gemini, Claude, etc.)
├── aagt-macros/        # Helper macros for defining tools
├── ARCHITECTURE.md     # Detailed architecture documentation
└── README.md           # This file
```

---

## 🌟 Why AAGT?

1.  **High Performance**: Rust-based with `async/await` for high concurrency.
2.  **Trading Native**: Built-in simulation, risk management, and strategy pipelines.
3.  **Easy Migration**: Use `simple_tool!` macro to convert existing Rust functions into AI-callable tools in minutes.
4.  **Production Ready**: Thread-safe, memory-efficient, and battle-tested in real trading scenarios.

---

---

## 💡 Use Cases

### Trading Bots
Build autonomous trading agents with built-in risk management and strategy execution.

### Social Media Agents
Create agents that interact with social platforms like [Moltbook](https://moltbook.com):
- Auto-post market insights
- Engage with community discussions
- Monitor sentiment and trends

**Example:** See [`examples/moltbook_agent.rs`](./examples/moltbook_agent.rs) for a complete social media agent implementation.

### Research Assistants
Deploy multi-agent swarms where specialized agents collaborate on complex research tasks.

### Customer Support
Build conversational agents with long-term memory to provide personalized support.

---

## 📖 Documentation

- [Architecture Guide](./ARCHITECTURE.md) - Detailed system design and component overview
- [API Reference](https://docs.rs/aagt-core) - Full API documentation (coming soon)

---

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

## 🙏 Acknowledgments

Built with ❤️ using Rust and inspired by the need for high-performance AI agents in trading environments.
