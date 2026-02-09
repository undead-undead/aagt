# AAGT Testing & Simulation Guide (v0.3.5)

This document provides a comprehensive overview of the testing and simulation capabilities within the AAGT framework. These features allow developers to verify complex agent behaviors, tool-calling logic, and third-party skill integrations without requiring live API keys or external services.

---

## 1. Advanced Simulation: MockProvider

The `MockProvider` (found in `aagt-providers`) is the cornerstone of AAGT's offline testing strategy. It simulates the behavior of an LLM, allowing for deterministic and reproducible test scenarios.

### 1.1 Key Features
- **Deterministic Tool Injection**: Manually specify when and how the model should trigger specific tools via `with_tool_calls`.
- **Infinite Loop Protection**: Built-in logic identifies tool result messages and automatically generates "processed" responses to gracefully close reasoning loops.
- **Realistic Streaming**: Simulates chunked, asynchronous responses to ensure your UI and event handlers handle streaming correctly.

### 1.2 Usage Example
```rust
use aagt_providers::mock::MockProvider;
use serde_json::json;

// Simulate a scenario where the model decides to use a weather tool
let provider = MockProvider::with_tool_calls(
    "I need to check the weather first...",
    vec![("call_abc".into(), "get_weather".into(), json!({"city": "Tokyo"}))]
);

// Inject this provider into your Agent
let agent = Agent::builder(provider).build()?;
```

---

## 2. Zero-Config Dynamic Skills

AAGT features an **Intelligent Inference** engine that allows it to load and execute skills from the ecosystem (e.g., ClawHub) even if they lack specific AAGT metadata.

### 2.1 Inference Logic
If a `SKILL.md` file is missing `script` or `runtime` fields, the `SkillLoader` automatically:
1. **Sniffs the File System**: Scans the `scripts/` directory for the first available script file.
2. **Auto-Detects Runtime**: Identifies the execution environment based on file extensions:
   - `.py` → **Python3**
   - `.js` → **Node.js**
   - `.sh` → **Bash**
3. **Parses Call Commands**: Scans Markdown code blocks for execution patterns if file detection fails.

### 2.2 Verifying Third-Party Skills
You can verify compatibility with any market skill (e.g., `openclaw-memory-audit`) by running a local integration test:
```bash
# Allow script execution without the Bubblewrap sandbox for local testing
export AAGT_UNSAFE_SKILL_EXEC=true
cargo test --test external_skill_test
```

---

## 3. Integration Testing Best Practices

We recommend placing integration tests in the `tests/` directory to keep business logic clean.

### 3.1 Recommended Workflow
1. **Define the Scenario**: Use `MockProvider` to define the expected model output.
2. **Instance Real Tools**: Import and register the actual `Tool` implementations used in your production code.
3. **Assert the Outcome**: Run `agent.prompt()` and verify that the final response or the tool execution state matches your expectations.

### 3.2 Reference Tests
- `aagt-providers/tests/moltbook_mock_test.rs`: Demonstrates a complex multi-turn registration flow.
- `aagt-core/tests/external_skill_test.rs`: Demonstrates loading and executing a skill package dynamically from the disk.

---

## 4. Security & Environment Notes

- **CI/CD Friendly**: Use `MockProvider` in your CI/CD pipelines to ensure logical correctness without token costs.
- **Sandboxing**: The `AAGT_UNSAFE_SKILL_EXEC` override should **NEVER** be used in production. It is intended strictly for development environments where `bwrap` (Bubblewrap) might not be available.
- **Hot Reloading**: `SkillLoader::load_all()` can be used to refresh the skill registry at runtime without restarting the agent service.
