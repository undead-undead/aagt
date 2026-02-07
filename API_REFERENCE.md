# AAGT API Reference (v0.3.0): Autonomous Agent Governance & Transport

AAGT is a modular framework organized into distinct layers, balancing core stability with high-speed extension capabilities for autonomous agents.

---

## Layer 1: Core Orchestration (aagt-core)

The heart of the framework, providing the central execution and coordination logic.

### AgentBuilder<P>
Fluent interface for agent construction.
- `new(provider: P)`: Initialize with an LLM provider.
- `session_id(id)`: Set a unique identifier for state persistence.
- `with_code_interpreter()`: Attaches the Python Sidecar (gRPC).
- `with_memory(memory)`: Binds a specific memory implementation (Hot + Cold).
- `with_tool(tool)`: Register a Rust-native tool.

### Agent<P>
The primary execution engine.
- `prompt(&self, text)`: High-level single-turn interaction.
- `chat(&self, messages)`: State-aware multi-turn conversation.
- `checkpoint(&self, messages, step, status)`: Save current cognitive context to persistent storage.
- `resume(&self, session_id)`: Reload and continue a previously saved session.

---

## Layer 2: State & Memory (aagt-core::agent::memory)

High-performance storage for conversation and domain knowledge.

### Memory Trait
Standardized interface for all storage backends.
- `store_session(session)`: Persist an AgentSession object.
- `retrieve_session(id)`: Fetch a saved session by ID.
- `search(query, limit)`: Unified semantic/text search.

### AgentSession
The serializable state of an agent.
- `messages`: Dialogue history.
- `step`: Current reasoning step index.
- `status`: Lifecycle state (Thinking, AwaitingApproval, Executing, etc.).

---

## Layer 3: Capability Runtimes (aagt-core::skills)

Bridges the framework to external execution environments.

### WasmRuntime
High-performance Sandbox for skills.
- **Isolation**: Memory-safe execution using Wasmtime.
- **Protocol**: Robust JSON-based parameter passing via shared memory.
- **Portability**: Skills can be compiled from any Wasm-compatible language.

### Sidecar (gRPC)
- Stateful code interpreter running in a dedicated Python ipykernel.
- Supports persistent variables and library access (LangChain, Pandas).

---

## Layer 4: Infrastructure & Integrations

### MessageBus (aagt-core::bus)
Asynchronous message router for multi-agent/multi-channel coordination.
- `subscribe(id, handler)`: Register an agent or channel.
- `publish(msg)`: Broadcast a message to the target destination.

### TelegramNotifier (aagt-core::infra)
- One-way notification channel via Telegram Bot API.

---

## Layer 5: Trading & Guardrails (aagt-core::trading)

Domain-specific logic for secure quant execution.

### RiskManager
The safety filter for every action.
- `check_and_reserve()`: Atomic quota reservation (Daily Volume, Single Trade).
- `commit_trade()` / `rollback_trade()`: Confirmation step for financial safety.

---

## Framework Specifications

- **Crates**: aagt-core, aagt-qmd, aagt-providers, aagt-macros.
- **Concurrency**: Async-first (Tokio), Thread-safe trait implementations.
- **Storage**: SQLite FTS5 + Vector (aagt-qmd) / In-memory (STM).
