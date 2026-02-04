# AAGT: Advanced Agentic Trading Framework

A high-performance, production-ready Rust framework for building autonomous trading agents with advanced memory, risk management, and multi-provider support.

---

## 🏗️ System Architecture

AAGT is built on a modular "Brain-Body-Nervous System" architecture designed for high reliability and multi-tenant security.

### 1. The Brain (Agent & Provider System)
- **Pluggable Intelligence**: Native support for Anthropic, OpenAI, Gemini, DeepSeek, Moonshot, and OpenRouter via a unified `Provider` trait.
- **Quota Protection**: Built-in fuses (`max_history_messages`, `max_tool_output_chars`) to prevent token bloat and control costs.
- **Context Management**: Advanced sliding window history management to keep reasoning sharp and cost-effective.

### 2. The Memory (Dual-Layer Persistence)
- **Context Layer (Short-Term)**: Atomic, persistent JSON snapshotting with `undo` capability. Prevents state corruption during VPS restarts.
- **Knowledge Layer (Long-Term)**: Append-only JSONL storage with metadata filtering for secure, multi-user RAG (Retrieval Augmented Generation).
- **Isolation Engine**: Strict logical and physical data separation between different User IDs and Agent IDs.

### 3. The Guardrails (Risk & Policy)
- **Risk Management**: Pluggable safety checks (Transaction limits, Volume caps, Honeypot detection).
- **Tool Policies**: Fine-grained execution control (Auto-run vs. Requires-Human-Approval).
- **Safety**: Built on Rust's type-safety and ownership model to prevent common concurrency bugs in high-frequency trading.

### 4. Integration & Automation
- **Strategy Pipeline**: Decoupled Detection -> Analysis -> Execution workflow.
- **Skill System**: Expand agent capabilities via simple Rust functions using the `#[tool]` macro.
- **Notification Bus**: Real-time alerts via Telegram, Discord, and Webhooks.

---

## 📚 Documentation & API

AAGT follows a strictly decoupled design. For detailed information on specific interfaces, methods, and configurations, please refer to our comprehensive API documentation:

👉 **[Download / View API Reference (API_REFERENCE.md)](./API_REFERENCE.md)**

### Key API Sections inside the Reference:
- **[Core Agent API](./API_REFERENCE.md#1-core-module-aagt-core)**: Building and running agents.
- **[Memory & Persistence](./API_REFERENCE.md#2-memory-module)**: Managing state and long-term knowledge.
- **[Risk Control](./API_REFERENCE.md#5-risk-management-riskmanager)**: Implementing trading safety checks.
- **[Multi-Agent Coordination](./API_REFERENCE.md#7-multi-agent-system)**: Orchestrating multiple expert agents.

---

## 🚀 Getting Started

```rust
use aagt_core::prelude::*;
use aagt_providers::openai::OpenAI;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Create a Provider
    let provider = OpenAI::from_env()?;

    // 2. Build an Agent with Quota Protection
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .system_prompt("You are a expert Solana trader.")
        .max_history_messages(10)
        .build()?;

    // 3. Start Chatting
    let response = agent.prompt("Check SOL price and analyze the trend.").await?;
    println!("Agent: {}", response);

    Ok(())
}
```

---

## ☕ Support the Project

If you find AAGT useful, consider supporting the developers:

**Buy Me a Coffee**: https://buymeacoffee.com/undeadundead

**Crypto Donations**:
- **Solana**: `9QFKQ3jpBSuNPLZQH1uq5GrJm4RDKue82zeVaXwazcmj`
- **Base**: `0x4cf0b79aea1c229dfb1df9e2b40ea5dd04f37969`

---

## ⚖️ License
MIT / Apache 2.0

---

**Built with Rust | Production-Ready v0.1.2**
