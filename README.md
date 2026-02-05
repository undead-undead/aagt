# AAGT: Advanced Agentic Trading Framework

A high-performance, production-ready Rust framework for building autonomous trading agents with advanced memory, risk management, and multi-provider support.

---

## System Architecture

AAGT is built on a modular "Brain-Body-Nervous System" architecture designed for high reliability and multi-tenant security.

### 1. The Brain (Agent & Provider System)
- **Pluggable Intelligence**: Native support for **8 providers** via a unified `Provider` trait:
  - **Cloud**: OpenAI, Anthropic, Gemini, DeepSeek 🇨🇳, Moonshot 🇨🇳, OpenRouter
  - **Groq** ⚡ - Ultra-fast inference (0.5s response) for real-time trading decisions
  - **Ollama** 🔐 - Local execution for complete privacy and zero API costs
- **Quota Protection**: Built-in fuses (`max_history_messages`, `max_tool_output_chars`) to prevent token bloat and control costs.
- **Context Management**: Advanced sliding window history management to keep reasoning sharp and cost-effective.

### 2. The Memory (Dual-Layer Persistence)
- **Context Layer (Short-Term)**: **RAM + Atomic JSON** - Fault-tolerant conversational state:
  - **Microsecond Access**: In-memory `DashMap` storage for zero-latency dialogue.
  - **Crash Safety**: Atomic write-and-rename strategy guarantees zero data loss on power failure.
  - **Auto-Recovery**: Instantly restores active sessions upon restart.
- **Knowledge Layer (Long-Term)**: **aagt-qmd** - High-performance hybrid search engine:
  - **100x faster search** (5ms vs 500ms for 100K documents)
  - **BM25 + Vector** hybrid retrieval (SQLite FTS5 + optional HNSW)
  - **25% storage savings** via content-addressable deduplication
  - **Token Efficient**: Replaces massive context windows with precise, relevance-based retrieval (~90% token savings).
  - **Zero cloud dependencies** - runs completely locally.
- **Isolation Engine**: Strict logical and physical data separation between different User IDs and Agent IDs.
- **Memory Tools**: Agents can actively `search_history` and `remember_this` for autonomous knowledge management.

### 3. The Guardrails (Risk & Policy)
- **Risk Management**: Pluggable safety checks (Transaction limits, Volume caps, Honeypot detection).
- **Tool Policies**: Fine-grained execution control (Auto-run vs. Requires-Human-Approval).
- **Safety**: Built on Rust's type-safety and ownership model to prevent common concurrency bugs in high-frequency trading.

### 4. Integration & Automation
- **Strategy Pipeline**: Decoupled Detection -> Analysis -> Execution workflow.
- **Skill System**: Expand agent capabilities via simple Rust functions using the `#[tool]` macro.
- **Notification Bus**: Real-time alerts via Telegram, Discord, and Webhooks.

---

## Documentation & API

AAGT follows a strictly decoupled design. For detailed information on specific interfaces, methods, and configurations, please refer to our comprehensive API documentation:

**[Download / View API Reference (API_REFERENCE.md)](./API_REFERENCE.md)**

### Key API Sections inside the Reference:
- **[Core Agent API](./API_REFERENCE.md#1-core-module-aagt-core)**: Building and running agents.
- **[Memory & Persistence](./API_REFERENCE.md#2-memory-module)**: Managing state and long-term knowledge.
- **[Risk Control](./API_REFERENCE.md#5-risk-management-riskmanager)**: Implementing trading safety checks.
- **[Multi-Agent Coordination](./API_REFERENCE.md#7-multi-agent-system)**: Orchestrating multiple expert agents.

---

## Getting Started

```rust
use aagt_core::prelude::*;
use aagt_providers::openai::OpenAI;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Create a Provider
    let provider = OpenAI::from_env()?;

    // 2. Setup Memory (backed by aagt-qmd hybrid search)
    let memory = Arc::new(MemoryManager::with_qmd("data/memory").await?);

    // 3. Build an Agent with Memory & Quota Protection
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .system_prompt("You are a expert Solana trader.")
        .max_history_messages(10)
        .with_memory(memory)  // Adds hybrid search memory
        .build()?;

    // 4. Start Chatting - Agent can now search history and save memories
    let response = agent.prompt("Check SOL price and analyze the trend.").await?;
    println!("Agent: {}", response);

    Ok(())
}
```

---

## Support the Project

If you find AAGT useful, consider supporting the developers:

**Buy Me a Coffee**: https://buymeacoffee.com/undeadundead

**Crypto Donations**:
- **Solana**: `9QFKQ3jpBSuNPLZQH1uq5GrJm4RDKue82zeVaXwazcmj`
- **Base**: `0x4cf0b79aea1c229dfb1df9e2b40ea5dd04f37969`

---

## License
MIT / Apache 2.0

---

**Built with Rust | Production-Ready v0.1.3**
