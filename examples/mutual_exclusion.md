# Mutual Exclusion Implementation Example

## ✅ Production Build (DynamicSkill Only)

```rust
use aagt_core::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let provider = OpenAIProvider::new(std::env::var("OPENAI_API_KEY")?);
    
    // Load DynamicSkills from ClawHub
    let skill_loader = Arc::new(SkillLoader::new("skills"));
    skill_loader.load_all().await?;
    
    // Build agent with DynamicSkill (secure, sandboxed)
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .with_dynamic_skills(skill_loader)? // ✅ SECURE
        // .with_code_interpreter("localhost:50051").await? // ❌ Would fail!
        .build()?;
    
    Ok(())
}
```

**Result**: Agent can use ClawHub skills in bwrap sandbox. Python Sidecar is disabled.

---

## ✅ Research Build (Python Sidecar Only)

```rust
use aagt_core::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let provider = AnthropicProvider::new(std::env::var("ANTHROPIC_API_KEY")?);
    
    // Build agent with Python Sidecar (exploratory analysis)
    let agent = Agent::builder(provider)
        .model("claude-3-5-sonnet-20241022")
        .with_code_interpreter("localhost:50051").await? // ✅ ENABLED
        // .with_dynamic_skills(skill_loader)?  // ❌ Would fail!
        .build()?;
    
    Ok(())
}
```

**Result**: Agent can execute LLM-generated Python code in Sidecar. DynamicSkill is disabled.

---

## ❌ ILLEGAL: Both Enabled (Compile Error)

```rust
use aagt_core::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let provider = OpenAIProvider::new(std::env::var("OPENAI_API_KEY")?);
    
    let skill_loader = Arc::new(SkillLoader::new("skills"));
    skill_loader.load_all().await?;
    
    let agent = Agent::builder(provider)
        .with_dynamic_skills(skill_loader)?      // First: DynamicSkill enabled
        .with_code_interpreter("localhost:50051").await?; // ❌ PANIC!
    
    // ERROR: Security Error: Cannot enable Python Sidecar when DynamicSkill is configured.
    //        These are mutually exclusive due to context pollution risks.
    //        See SECURITY.md for details.
    
    Ok(())
}
```

**Result**: Compilation fails with clear error message explaining security rationale.

---

## Error Messages

### Case 1: Enable Sidecar after DynamicSkill

```rust
let agent = Agent::builder(provider)
    .with_dynamic_skills(loader)?                // ✅ OK: DynamicSkill first
    .with_code_interpreter("localhost:50051").await?; // ❌ ERROR
```

**Error**:
```
Error: agent_config("Security Error: Cannot enable Python Sidecar when DynamicSkill is configured. \
These are mutually exclusive due to context pollution risks. See SECURITY.md for details.")
```

---

### Case 2: Enable DynamicSkill after Sidecar

```rust
let agent = Agent::builder(provider)
    .with_code_interpreter("localhost:50051").await?  // ✅ OK: Sidecar first
    .with_dynamic_skills(loader)?;                    // ❌ ERROR
```

**Error**:
```
Error: agent_config("Security Error: Cannot enable DynamicSkill when Python Sidecar is configured. \
These are mutually exclusive due to context pollution risks. See SECURITY.md for details.")
```

---

## Enforcement Mechanism

The `AgentBuilder` tracks which execution model is enabled:

```rust
pub struct AgentBuilder<P: Provider> {
    // ... other fields
    
    /// Security: Track if Python Sidecar is enabled
    has_sidecar: bool,
    
    /// Security: Track if DynamicSkill is enabled
    has_dynamic_skill: bool,
}
```

**Order of calls doesn't matter** - whichever you configure first "locks in" the mode.

---

## Benefits

1. **Compile-Time Safety**: Developers cannot accidentally create vulnerable configurations
2. **Clear Error Messages**: If misconfigured, users immediately know why and how to fix
3. **Documentation as Code**: The method signatures document the security constraint
4. **Fail-Fast**: Catches errors at build time, not production runtime

---

## Migration Guide

### If you previously had:

```rust
// Old code (vulnerable!)
let agent = Agent::builder(provider)
    .tool(my_dynamic_skill)  // Manual tool addition
    .with_code_interpreter("localhost:50051").await?
    .build()?;
```

### Change to (Production):

```rust
// New code (secure)
let skill_loader = Arc::new(SkillLoader::new("skills"));
skill_loader.load_all().await?;

let agent = Agent::builder(provider)
    .with_dynamic_skills(skill_loader)?  // Replaces manual .tool() calls
    .build()?;
```

### Or change to (Research):

```rust
// New code (secure, no skills)
let agent = Agent::builder(provider)
    .with_code_interpreter("localhost:50051").await?
    .build()?;

// DO NOT install ClawHub skills in this mode!
```

---

## See Also

- [SECURITY.md](../SECURITY.md) - Detailed threat model
- [CONFIGURATION.md](../CONFIGURATION.md) - Configuration guidelines
- [API_REFERENCE.md](../API_REFERENCE.md) - API documentation
