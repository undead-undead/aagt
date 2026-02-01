# Contributing to AAGT

Thank you for your interest in contributing to AAGT! This document provides guidelines and instructions for contributing.

---

## Code of Conduct

Be respectful, constructive, and professional in all interactions.

---

## How to Contribute

### Reporting Bugs

1. Check if the issue already exists in [GitHub Issues](https://github.com/undead-undead/aagt/issues)
2. If not, create a new issue with:
   - Clear title and description
   - Steps to reproduce
   - Expected vs actual behavior
   - Rust version and OS

### Suggesting Features

1. Open a GitHub Issue with the `enhancement` label
2. Describe the feature and its use case
3. Provide examples if possible

### Submitting Pull Requests

1. **Fork the repository**
2. **Create a feature branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Make your changes**:
   - Follow the code style guidelines below
   - Add tests for new functionality
   - Update documentation as needed
4. **Run tests**:
   ```bash
   cargo test --all
   ```
5. **Run lints**:
   ```bash
   cargo clippy --all-targets --all-features
   cargo fmt --all -- --check
   ```
6. **Commit your changes**:
   ```bash
   git commit -m "feat: add new feature"
   ```
   Use [Conventional Commits](https://www.conventionalcommits.org/) format:
   - `feat:` for new features
   - `fix:` for bug fixes
   - `docs:` for documentation changes
   - `refactor:` for code refactoring
   - `test:` for adding tests
7. **Push to your fork**:
   ```bash
   git push origin feature/your-feature-name
   ```
8. **Open a Pull Request** on GitHub

---

## Code Style Guidelines

### Rust Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Avoid `unwrap()` and `expect()` in library code (use `?` or `Result`)

### Naming Conventions

- **Types**: `PascalCase` (e.g., `Agent`, `ToolDefinition`)
- **Functions**: `snake_case` (e.g., `stream_completion`, `add_tool`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `GEMINI_2_0_FLASH`)

### Documentation

- Add doc comments (`///`) for all public items
- Include examples in doc comments where helpful:
  ```rust
  /// Adds a tool to the agent.
  ///
  /// # Example
  /// ```
  /// agent.add_tool(Box::new(MyTool));
  /// ```
  pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
      // ...
  }
  ```

### Error Handling

- Use `thiserror` for library errors
- Use `anyhow` for application errors
- Provide context in error messages:
  ```rust
  .map_err(|e| anyhow::anyhow!("Failed to fetch price for {}: {}", symbol, e))?
  ```

---

## Project Structure

```
aagt/
├── aagt-core/          # Core abstractions
│   ├── src/
│   │   ├── agent.rs    # Agent engine
│   │   ├── tool.rs     # Tool system
│   │   ├── provider.rs # Provider trait
│   │   └── ...
│   └── Cargo.toml
├── aagt-providers/     # LLM implementations
│   ├── src/
│   │   ├── openai.rs
│   │   ├── gemini.rs
│   │   └── ...
│   └── Cargo.toml
├── aagt-macros/        # Procedural macros
│   └── src/lib.rs
└── Cargo.toml          # Workspace config
```

---

## Testing

### Unit Tests

Place unit tests in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_builder() {
        // ...
    }
}
```

### Integration Tests

Place integration tests in `tests/` directory:

```rust
// tests/agent_integration.rs
use aagt_core::prelude::*;

#[tokio::test]
async fn test_full_workflow() {
    // ...
}
```

### Running Tests

```bash
# Run all tests
cargo test --all

# Run specific test
cargo test test_agent_builder

# Run with output
cargo test -- --nocapture
```

---

## Adding a New LLM Provider

1. Create a new file in `aagt-providers/src/` (e.g., `mistral.rs`)
2. Implement the `Provider` trait:
   ```rust
   use aagt_core::provider::Provider;
   use async_trait::async_trait;

   pub struct Mistral {
       api_key: String,
       client: reqwest::Client,
   }

   #[async_trait]
   impl Provider for Mistral {
       async fn stream_completion(
           &self,
           model: &str,
           system_prompt: Option<&str>,
           messages: Vec<Message>,
           tools: Vec<ToolDefinition>,
           config: CompletionConfig,
       ) -> Result<StreamingResponse> {
           // Implementation
       }
   }
   ```
3. Add tests
4. Update documentation
5. Submit a PR

---

## Release Process

*(For maintainers)*

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create a git tag:
   ```bash
   git tag -a v0.2.0 -m "Release v0.2.0"
   git push origin v0.2.0
   ```
4. Publish to crates.io:
   ```bash
   cargo publish -p aagt-core
   cargo publish -p aagt-providers
   ```

---

## Questions?

Open a [GitHub Discussion](https://github.com/undead-undead/aagt/discussions) or reach out to the maintainers.

---

Thank you for contributing! 🎉
