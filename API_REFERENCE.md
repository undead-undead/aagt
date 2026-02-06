# AAGT API Reference (v0.2.3)

AAGT is a modular framework organized into distinct layers, balancing core stability with high-speed extension capabilities.

---

## 🏗️ Layer 1: Core Orchestration (`aagt-core`)

The heart of the framework, providing the central execution and coordination logic.

### `AgentBuilder<P>`
Fluent interface for agent construction.
- `new(provider: P)`: Initialize with an LLM provider.
- `model(name)` / `system_prompt(text)` / `temperature(f64)`.
- `with_code_interpreter()`: Attaches the Python Sidecar (gRPC).
- `with_memory_path(path)`: Connects the Tiered Memory system (Hot + Cold).
- `with_message_bus(bus)`: [NEW] Subscribes the agent to a unified event stream.
- `tool(T)` / `link_skill_loader(L)`: Add Rust-native or dynamic Python skills.

### `Agent<P>`
The primary executor.
- `prompt(&self, text)`: High-level single-turn interaction.
- `chat(&self, messages)`: State-aware multi-turn conversation.
- `stream(&self, text)`: Real-time token streaming.
- `handler()`: Returns a `MessageBus` compatible async handler.

---

## 📡 Layer 2: Message Routing (`aagt-core::bus`) [NEW]

Unified event-driven communication layer.

### `MessageBus`
Asynchronous message router for multi-agent/multi-channel coordination.
- `subscribe(id, handler)`: Register an agent or channel.
- `publish(msg)`: Broadcast a message to the target destination.

### `InboundMessage` / `OutboundMessage`
Standardized schemas for agent communication, supporting text and media attachments.

---

## 🧠 Layer 3: State & Memory (`aagt-core::knowledge`)

High-performance storage for conversation and domain knowledge.

### `MemoryManager` (Tiered Storage)
- **Hot Tier (STM)**: Microsecond access via in-memory DashMap, persisted to atomic JSON.
- **Cold Tier (LTM)**: Content-Addressable aagt-qmd engine (SQLite + Vector).
- `retrieve_unified()`: Automatically merges history from both tiers.

### `NamespacedMemory` [NEW]
Isolation layer for specific data categories with TTL support.
- `store(ns, key, val, ttl)`: Save data into a protected namespace (e.g., "market_depth").
- `read(ns, key)`: Retrieval with automatic expiration checks.

### `Union Search`
- `search_unified()`: Simultaneous BM25 and Vector search with RRF re-ranking.

---

## 🛠️ Layer 4: Capability & Infrastructure (`aagt-core::skills`)

Bridges the framework to external environments.

### `Sidecar` (gRPC Capability)
- Stateful code interpreter running in a dedicated Python ipykernel.
- Supports persistent variables and library access (LangChain, Pandas).

### `DynamicSkill` & `ClawHub`
- `ClawHub`: Auto-discovery and loading of MCP-compatible Python tools.
- **Security**: Sidecar and DynamicSkill are mutually exclusive per agent instance to prevent context pollution.

### `TelegramNotifier` (`aagt-core::infra`) [NEW]
- One-way notification channel via Telegram Bot API.
- Integrated into the framework's alert system.

---

## 🛡️ Layer 5: Guardrails & Strategy (`aagt-core::trading`)

Domain-specific logic for secure quant execution.

### `RiskManager`
The safety filter for every action.
- `check_and_reserve()`: Atomic quota reservation (Daily Volume, Single Trade).
- `commit_trade()` / `rollback_trade()`: State confirmation for financial safety.

### `Strategy` & `Pipeline`
- Standardized loop for Market Detection ➔ AI Analysis ➔ Risk Check ➔ Execution.

---

## 🔌 Framework Specifications

- **Crates**: `aagt-core`, `aagt-qmd`, `aagt-providers`, `aagt-macros`.
- **Concurrency**: Async-first (Tokio), Thread-safe trait implementations.
- **Distribution**: Licensed under MIT/Apache 2.0.

