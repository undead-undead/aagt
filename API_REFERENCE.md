# AAGT API Reference

AAGT (Advanced Agentic Trading) is organized into logical layers, balancing core stability with dynamic extension capabilities.

---

## Layer 1: Core Intelligence (aagt-core)
The primarily interface for building and running AI Agents.

### AgentBuilder<P>
Fluent interface for agent construction.
- new(provider: P): Start a builder.
- model(name) / system_prompt(text) / temperature(f64) / max_tokens(u64).
- max_history_messages(n): Sets the sliding window size.
- with_code_interpreter(): [NEW] Attaches the Python Sidecar capability.
- with_memory_path(path): Connects the aagt-qmd hybrid search engine.
- tool(T): Add a Rust-native tool (Skill).
- approval_handler(H) / notifier(N).

### Agent<P>
The central executor.
- prompt(&self, text) -> anyhow::Result<String>: Send a single prompt.
- chat(&self, messages: Vec<Message>) -> anyhow::Result<String>: Multi-turn chat.
- stream(&self, text) -> Result<StreamingResponse>: Real-time token streaming.
- undo(user_id, agent_id): Roll back the last interaction atomically.

---

## Layer 2: Capability & Extension
Bridges the Rust core to the external world via gRPC and WASM.

### Sidecar (gRPC Capability)
Stateful, rich execution in Python.
- SidecarManager: Manages the lifecycle of the Python ipykernel subprocess.
- Sidecar::execute(code): Execute Python code and receive stdout, stderr, and images.

### WasmRuntime (WASM Capability)
Hot-swappable plugins for dynamic skills.
- WasmRuntime::new(bytes): Initialize a WASM engine (Wasmtime).
- WasmRuntime::call(args): Execute guest code in a WASI sandbox.

### DynamicSkill
Universal wrapper for python3, node, bash, and wasm skill types.

---

## Layer 3: State & Memory
Managing short-term context and long-term knowledge.

### ContextManager
Gatekeeper for the LLM context window.
- build_context(history): Applies RAG, windowing, and ContextInjector data.

### MemoryManager
- with_qmd(path): Initialize with Hybrid Search storage.
- search_history (Tool): Agent-accessible BM25 history search.
- remember_this (Tool): Agent-accessible insights persistence.

### aagt-qmd (Hybrid Search Engine)
- HybridSearchEngine: SQLite FTS5 (BM25) + optional HNSW (Vector).
- engine.search(query, limit): Unified RRF retrieval.

---

## 🛡️ Layer 4: Guardrails & Strategy (`RiskManager`)
Ensuring trading safety and automated execution patterns.

### `RiskManager`
The safety filter for every action.
- `RiskCheck` Trait: `check(context)`, `commit(context)`, `rollback(context)`.
- **Built-in Checks**: `SingleTradeLimit`, `DailyVolumeLimit`, `TokenSecurity`, `SlippageCheck`.

### `Strategy` & `Pipeline`
- `Strategy::new(action, executor)`.
- `PriceTrigger` / `CronTrigger`.
- `Pipeline`: Orchestrates Detection ➔ Analysis ➔ Risk Check ➔ Execution.

---

## 📡 Layer 5: Orchestration & UI
Multi-agent coordination and event notifications.

### `Coordinator` (Multi-Agent)
- `add_agent(role, agent)`.
- `delegate(target, task)`: Inter-agent task handoff.
- `broadcast(message)`.

### `Notifier` & `Events`
- **Channels**: `Telegram`, `Discord`, `Email`, `Webhook`, `Terminal`.
- **AgentEvents**: `Thinking`, `ToolCall`, `ApprovalPending`, `Response`, `Error`.

---

## 🔌 Architecture Specs
- **Crates**: `aagt-core`, `aagt-qmd`, `aagt-providers`, `aagt-macros`, `aagt-sidecar`.
- **Patterns**: Strategy (Traits), Builder (Config), Facade (Agent), Bridge (gRPC).
