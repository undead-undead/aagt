# AAGT: Autonomous Agent Governance & Transport

> **The High-Performance Infrastructure for Resilient AI Agents.**

AAGT is a next-generation application framework designed for building autonomous, durable, and secure agent systems. Built on Rust, it bridges the gap between high-speed asynchronous execution and complex stateful reasoning. By integrating Wasm-based skill sandboxing, a persistent cognitive state machine, and a unified message bus, AAGT provides the industrial-grade "Governance and Transport" layer required for production AI deployments.

---

## Core Pillars

AAGT is built on a modular architecture designed for extreme reliability and real-time orchestration.

### 1. High-Performance Execution (Rust Core)
Rust's type-safety and ownership model provide the foundation for execution and safety.
- **Zero-Latency**: Async-first design (Tokio) with microsecond overhead.
- **Parallel Tool Calling**: Concurrent execution of multiple tools with configurable limits.
- **Risk Management**: Pluggable RiskManager with atomic quota reservation and trade validation.

### 2. Isolated Capability Runtimes
Extend agent capabilities without compromising system stability.
- **Wasm Runtime**: Sandboxed, high-performance skills written in any Wasm-compatible language (Rust, Go, etc.) with per-skill security policies.
- **Python Sidecar**: gRPC-linked ipykernel for heavy data analysis, LangChain integration, and persistent Jupyter-like logic.

### 3. Persistent State Machine
Agents that survive restarts and handle long-running reasoning loops.
- **SQLite Serialization**: Full cognitive state (dialogue history, reasoning steps, tool status) persisted to disk.
- **Suspend & Resume**: Native support for human-in-the-loop (HITL) workflows where agents pause for approval and resume seamlessly.

### 4. Tiered Memory System (aagt-qmd)
Content-addressable hybrid search engine for deep historical reasoning.
- **Tiered Search**: Seamlessly bridges Short-Term (Hot) and Long-Term (Cold/Vector) tiers.
- **Semantic Context**: Automatic retrieval of relevant documents via BM25 and Vector Search re-ranking.
- **Active Indexing**: Real-time background indexing of files and conversation history.

---

## Security & Reliability

- **Policy-Driven Execution**: Configurable policies (Auto, RequiresApproval, Disabled) for every tool and skill.
- **Binary Sandbox**: Strict syscall and capability filters for binary skills.
- **Resource Monitoring**: Built-in tracking of RAM, Disk, and Token usage.

---

## Multi-Provider Support

Native support for LLMs via a unified Provider trait:
- OpenAI, Anthropic (Claude), Gemini, groq, DeepSeek, Ollama.

---

## Quick Start (v0.3.0)

```rust
use aagt_core::prelude::*;
use aagt_providers::openai::OpenAI;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Components
    let provider = OpenAI::from_env()?;

    // 2. Build Agent with Persistence & session management
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .session_id("agent-session-001")
        .with_memory_path("data/storage.db") // Persistent SQLite + Vector tier
        .build()?;

    // 3. Start Execution
    let response = agent.prompt("Analyze the provided dataset and suggest an optimized workflow.").await?;
    println!("Agent response: {}", response);

    Ok(())
}
```

---

## Architecture Detail

| Component | Description |
|-----------|-------------|
| aagt-core | The heart of the framework: orchestration, traits, and agent loops. |
| aagt-qmd | High-performance memory engine (SQLite FTS5 + Vector). |
| aagt-providers | Unified interface for LLM providers. |
| aagt-macros | Procedural macros for developer ergonomics. |

---

## Support & License

- **License**: MIT / Apache 2.0
- **Donate**: 
  - **Solana**: `9QFKQ3jpBSuNPLZQH1uq5GrJm4RDKue82zeVaXwazcmj`
  - **Base**: `0x4cf0b79aea1c229dfb1df9e2b40ea5dd04f37969`
