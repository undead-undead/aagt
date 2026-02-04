# AAGT API Reference

This document provides a comprehensive overview of the public interfaces and APIs within the AAGT (Advanced Agentic Trading) framework.

---

## 1. Core Module (`aagt-core`)

### Agent System
The central entry point for creating and interacting with AI agents.

#### `AgentBuilder<P>`
Used to configure and construct an `Agent`.
- `new(provider: P)`: Initialize a new builder with a specific LLM provider.
- `model(name: String)`: Set the model ID (e.g., "gpt-4o", "claude-3-5-sonnet").
- `system_prompt(text: String)`: Set the base system instructions (Preamble).
- `temperature(value: f64)`: Set sampling temperature (0.0 - 1.0).
- `max_tokens(n: u64)`: Set response token limit.
- `max_history_messages(n: usize)`: Set the sliding window for conversation history.
- `max_tool_output_chars(n: usize)`: Set the hard limit for tool output length to save tokens.
- `tool(T)`: Add a tool/skill to the agent.
- `approval_handler(H)`: Set a handler for human-in-the-loop approvals.
- `notifier(N)`: Set a notification backend (Telegram, Discord, etc.).
- `build()`: Construct the `Agent` instance.

#### `Agent<P>`
The primary interface for execution.
- `prompt(text: String) -> Result<String>`: Send a single-turn prompt.
- `chat(messages: Vec<Message>) -> Result<String>`: Continue a conversation.
- `stream(text: String) -> Result<StreamingResponse>`: Get a streaming response.
- `undo(user_id, agent_id)`: Roll back the last interaction in memory.

---

## 2. Memory Module

### `Memory` Trait
The interface for all memory implementations.
- `store(user_id, agent_id, message)`: Persist a message.
- `retrieve(user_id, agent_id, limit)`: Fetch recent messages.
- `clear(user_id, agent_id)`: Wipe conversation history.
- `undo(user_id, agent_id)`: Remove the last stored message atomically.

#### `ShortTermMemory` (Context Persistence)
- **Mechanism**: Atomic Rename (Temp-then-Replace) JSON.
- **Auto-save**: Every write triggers an asynchronous flush to disk.
- **Isolation**: Physical path-based and Logical ID-based isolation.

#### `LongTermMemory` (Knowledge Base)
- **Mechanism**: Append-only JSONL with vector-like metadata search.
- **`store_entry(MemoryEntry)`**: Store a specific knowledge point or fact.
- **`retrieve_recent(user_id, agent_id, char_limit)`**: Optimized RAG retrieval.
- **`prune(limit, user_id)`**: Logic to maintain storage size constraints.

---

## 3. Providers (`aagt-providers`)

AAGT supports multiple LLM providers through a unified interface.

| Provider | Model Constants | Features |
| :--- | :--- | :--- |
| **OpenAI** | `GPT_4O`, `GPT_4O_MINI` | Tool calling, Parallel tools |
| **Anthropic** | `CLAUDE_3_5_SONNET`, `CLAUDE_3_HAIKU` | High reasoning, Tool use |
| **Gemini** | `GEMINI_2_0_FLASH`, `GEMINI_1_5_PRO` | Deep reasoning, Multimodal |
| **DeepSeek** | `DEEPSEEK_CHAT`, `DEEPSEEK_CODER` | Efficient trading analysis |
| **Moonshot** | `MOONSHOT_V1_8K` | Kimi compatibility |
| **OpenRouter** | Various | Unified routing to 100+ models |

---

## 4. Skills & Tools

### `Tool` Trait
- `name()`: Identifier for the LLM.
- `description()`: Instruction for the LLM on when to use it.
- `definition()`: JSON Schema of parameters.
- `call(arguments: &str)`: Execute the logic.

### Macros
- `#[tool]`: Procedural macro to generate a `Tool` from a standard Rust function.

---

## 5. Risk Management (`RiskManager`)

Enforces safety and limits on agent actions.

#### `RiskCheck` Trait
- `check(context: &TradeContext) -> Result<()>`: Validate an action.
- `commit(context: &TradeContext)`: Record a successful trade into risk state.
- `rollback(context: &TradeContext)`: Revert a pending trade state.

#### Built-in Checks
- `SingleTradeLimit`: Limits USD volume per trade.
- `DailyVolumeLimit`: Caps total trading volume in 24h.
- `TokenSecurity`: Blacklists/Whitelists specific tokens.
- `SlippageCheck`: Prevents execution if expected slippage is too high.
- `CompositeCheck`: Combines multiple checks into one logical unit.

---

## 6. Trading Automation

### `Strategy`
A high-level definition of trading logic.
- `new(action, executor)`: Create a strategy.
- `PriceTrigger`: Execute when a price crosses a threshold.
- `CronTrigger`: Execute on a schedule.

### `Pipeline`
Orchestrates the flow from Detection -> Analysis -> Risk Check -> Execution.
- `add_step(step)`: Append a processing step.
- `run(input)`: Execute the full sequence.

---

## 7. Multi-Agent System

### `Coordinator`
Manages multiple agents working together.
- `add_agent(role, agent)`: Assign an agent to a specific role.
- `broadcast(message)`: Send data to all managed agents.
- `delegate(target, task)`: Assign a sub-task from one agent to another.

---

## 8. Notifications & Events

### `Notifier` Trait
- `notify(channel, message)`: Send a message to the user.
- **Channels**: `Telegram`, `Discord`, `Email`, `Webhook`, `Terminal`.

### `AgentEvent`
Enum of events emitted via `agent.subscribe()`:
- `Thinking`: Start of generation.
- `ToolCall`: Tool usage detected.
- `ToolResult`: Tool results obtained.
- `ApprovalPending`: Hitting a `RequiresApproval` policy.
- `Response`: Final text output.
- `Error`: Something went wrong.

---

## 9. Simulation & Testing

### `MockProvider`
A provider that returns predefined responses for testing without API costs.

### `SimulationManager`
- `start()`: Begin a sandboxed run.
- `snapshot()`: Take a point-in-time state of the simulation.
