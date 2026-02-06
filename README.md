# AAGT: Advanced Agentic Trading Framework

> **The Rust-powered Application Framework for Autonomous Trading.**

[![Crates.io](https://img.shields.io/crates/v/aagt-core.svg)](https://crates.io/crates/aagt-core)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

AAGT is a high-performance, production-ready framework designed to solve the "Dynamic-Static Conflict" in AI Agents. It combines Rust's uncompromising safety and performance with Python's rich AI ecosystem, positioning itself as a **Domain-Specific Application Framework** for quant trading.

---

## 🏗️ System Architecture: The Quad-Core Engine

AAGT v0.2.3 is built on a modular architecture designed for extreme reliability and real-time orchestration.

### 1. The Guard (Rust Core)
Rust's type-safety and ownership model provide the foundation for execution and safety.
- **High-Performance**: Zero-cost abstractions and async-first design (Tokio).
- **Risk Management**: Pluggable `RiskManager` with atomic quota reservation.
- **Tiered Memory**: Hot (hot-swappable STM) and Cold (SQLite/Vector LTM) storage tiers.

### 2. The Thinker (Python gRPC Sidecar)
Offload intelligence to where it belongs—the world's most mature AI ecosystem.
- **Stateful Code Interpreter**: Integrated Jupyter ipykernel for persistent data analysis.
- **Ecosystem Integration**: Access LangChain, NumPy, and Pandas via high-speed gRPC.

### 3. The Router (Message Bus) [NEW]
Unified event routing for multi-channel communication.
- **Asynchronous mpsc**: High-throughput message queuing between agents and channels.
- **Unified Schema**: `InboundMessage` and `OutboundMessage` for seamless integration.

### 4. The Bridge (Infrastructure) [NEW]
Native integrations for real-world notifications and automated actions.
- **Telegram Notifier**: Direct HTTP API integration for one-way alerts.
- **Dynamic Skills**: Plugin-based tool system with security guardrails.

---

## 🧠 Memory System (aagt-qmd)

AAGT features a **Content-Addressable Hybrid Search** engine for deep historical reasoning:
- **Namespaced Memory**: Isolated storage for different data categories (market, news, analysis).
- **Union Search**: Simultaneous keyword and vector search across Short-Term and Long-Term tiers.
- **100x Faster Retrieval**: SQLite FTS5 backend ensures <5ms latency.

---

## 🛡️ Guardrails & Security

- **Risk Policies**: `DailyVolumeLimit`, `SingleTradeLimit`, and `SlippageCheck`.
- **Sandbox Isolation**: Sidecar and DynamicSkill execution with optional containerization.
- **Mutual Exclusion**: Enhanced security model preventing context pollution between sidecars and local skills.

---

## 🔌 Multi-Provider Support

Native support for LLMs via a unified `Provider` trait:
- OpenAI, Anthropic (Claude), Gemini, Groq, Groq, DeepSeek, and Ollama.

---

## 🚀 Quick Start (v0.2.3)

```rust
use aagt_core::prelude::*;
use aagt_core::bus::MessageBus;
use aagt_core::infra::TelegramNotifier;
use aagt_providers::openai::OpenAI;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Components
    let provider = OpenAI::from_env()?;
    let bus = MessageBus::new();
    let notifier = TelegramNotifier::new("TOKEN", "CHAT_ID");

    // 2. Build Agent with Unified Config
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .with_code_interpreter()      // Python sidecar enabled
        .with_memory_path("data/")    // Tiered RAG enabled
        .build()?;

    // 3. Connect to Message Bus
    bus.subscribe(agent.id(), agent.handler()).await;
    
    // 4. Start Execution
    let response = agent.prompt("Analyze BTC/USDT price action.").await?;
    notifier.notify(&format!("Agent Analysis: {}", response)).await?;

    Ok(())
}
```

---

## 📦 Packages

Available on [crates.io](https://crates.io/search?q=aagt):
- `aagt-core`: The heart of the framework.
- `aagt-providers`: LLM integrations.
- `aagt-qmd`: Memory and Search engine.
- `aagt-macros`: Developer ergonomics.

---

## 💖 Support & License
- **License**: MIT / Apache 2.0
- **Donate**: 
  - **Solana**: `9QFKQ3jpBSuNPLZQH1uq5GrJm4RDKue82zeVaXwazcmj`
  - **Base**: `0x4cf0b79aea1c229dfb1df9e2b40ea5dd04f37969`

**Built by Traders, for Developers.**

