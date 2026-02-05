# AAGT: Advanced Agentic Trading Framework

> **The Rust-powered Hybrid Intelligence System for Autonomous Trading.**

AAGT is a high-performance, production-ready framework designed to solve the "Dynamic-Static Conflict" in AI Agents. It combines Rust's uncompromising safety and performance with Python's rich AI ecosystem and WASM's dynamic agility.

---

## System Architecture: The Triple-Core Engine

AAGT is built on a modular "Triple-Core" architecture designed for high reliability, intelligence, and extreme extensibility.

### 1. The Guard (Rust Core) | *The Body*
Rust's type-safety and ownership model provide the foundation for execution and safety.
- **High-Performance**: Zero-cost abstractions and async-first design (Tokio) for low-latency decision making.
- **Risk Management**: Pluggable safety checks (RiskManager) with native code execution.
- **Context Management**: Advanced sliding window history management (ContextManager) to optimize reasoning and costs.

### 2. The Thinker (Python gRPC Sidecar) | *The Brain*
Offload intelligence to where it belongs—the world's most mature AI ecosystem.
- **Stateful Code Interpreter**: Integrated Jupyter ipykernel allows the agent to maintain variables and state across multiple execution cells.
- **Ecosystem Integration**: Seamless access to LangChain, NumPy, Pandas, and professional quantitative libraries via gRPC.
- **Zero-Block Execution**: Ensures heavy ML reasoning doesn't block the critical trading execution loop.

### 3. The Runner (WASM Runtime) | *The Reflex*
True dynamic extensibility without recompilation.
- **Hot-Pluggable Skills**: Write skills in any language (Rust, AssemblyScript, C++) and load them as .wasm files.
- **Sandboxed Security**: Third-party plugins run in a strictly isolated WASI environment with zero access to the host private keys or sensitive OS resources.

---

## Memory System (aagt-qmd)

AAGT features a **Content-Addressable Hybrid Search** engine for deep historical reasoning:
- **Dual-Layer Persistence**: 
  - **Short-Term**: Atomic JSON (Temp-then-Replace) for microsecond session recovery.
  - **Long-Term**: aagt-qmd Hybrid Search (BM25 + Vector) for token-efficient RAG (~90% savings).
- **100x Faster Retrieval**: SQLite FTS5 backend ensures 5ms searches even with 100K+ historical documents.
- **Privacy First**: Zero cloud dependencies. Your strategies and history stay on your infrastructure.

---

## Guardrails & Security

AAGT is "Safe-by-Design":
- **Risk Checks**: Built-in SingleTradeLimit, DailyVolumeLimit, HoneypotDetection, and SlippageCheck.
- **Approval Policies**: Fine-grained control with Auto-Run vs Human-in-the-Loop (via Telegram/Discord/Webhooks).
- **Isolation**: Physical and logical data separation between different User IDs and Agent IDs.

---

## Multi-Provider Support

Native support for LLMs via a unified Provider trait:
- **Cloud**: OpenAI, Anthropic (Claude), Gemini.
- **Fast Inference**: Groq (0.5s decision time).
- **Privacy & Local**: Ollama.
- **Open Standards**: OpenRouter, DeepSeek, Moonshot.

---

##  Quick Start

```rust
use aagt_core::prelude::*;
use aagt_providers::openai::OpenAI;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize Intelligence
    let provider = OpenAI::from_env()?;

    // 2. Build Agent with Hybrid Capabilities
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .with_code_interpreter()      // Stateful Python sidecar
        .with_wasm_skills("skills/")  // WASM hot-swappable plugins
        .with_memory_path("data/")    // Hybrid search RAG
        .build()?;

    // 3. Start Secure Execution
    let response = agent.prompt("Analyze SOL/USDT market depth and plot a 14-day RSI.").await?;
    println!("Agent Analysis: {}", response);

    Ok(())
}
```

---

## Support & License
- **License**: MIT / Apache 2.0
- **Donate**: 
  - **Solana**: `9QFKQ3jpBSuNPLZQH1uq5GrJm4RDKue82zeVaXwazcmj`
  - **Base**: `0x4cf0b79aea1c229dfb1df9e2b40ea5dd04f37969`

**Built by Traders, for Developers.**
