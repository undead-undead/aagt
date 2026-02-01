# AAGT (Advanced Agentic Trading) 架构设计方案

AAGT 是一个专为加密货币交易设计的轻量级、高性能 AI Agent 框架。它通过高度抽象的接口层，将复杂的 LLM 逻辑与具体的交易操作解耦。

---

## 一、 系统架构概览

AAGT 采用分层架构设计，从底层到顶层分为：适配层、核心抽象层、逻辑执行层。

### 1.1 架构图 (Mermaid)

```mermaid
graph TD
    %% 用户应用层
    subgraph App_Layer [应用层 / listen-kit]
        UserAgent[Agent Instance]
        Tools[Custom Tools]
        Loop[Reasoning Loop]
    end

    %% 核心抽象层
    subgraph Core_Layer [AAGT Core]
        AgentStruct[Agent Struct]
        ToolTrait[Tool Trait]
        ProviderTrait[Provider Trait]
        RiskManager[Risk Manager]
        MemoryMap[Memory System]
    end

    %% 模型适配层
    subgraph Provider_Layer [AAGT Providers]
        OpenAI[OpenAI / DeepSeek]
        Claude[Anthropic Claude]
        Gemini[Google Gemini]
        OpenRouter[OpenRouter]
    end

    %% 流程指向
    UserAgent --> AgentStruct
    AgentStruct -->|使用| ProviderTrait
    AgentStruct -->|注册| ToolTrait
    ProviderTrait <|-- OpenAI
    ProviderTrait <|-- Claude
    ProviderTrait <|-- Gemini
    ProviderTrait <|-- OpenRouter
    
    Loop -->|检查风险| RiskManager
    Loop -->|调用| ToolTrait
    Loop -->|读写| MemoryMap
```

---

## 二、 核心模块说明

### 2.1 Agent 引擎 (`aagt-core/agent.rs`)
Agent 是系统的核心枢纽，负责持有 `Provider` 和 `ToolSet`。
*   **AgentBuilder**: 采用构建者模式，支持动态注入 Prompt (Preamble)、模型参数（Temperature, Max Tokens）以及工具。
*   **Chat/Stream 接口**: 统一了普通请求与流式输出接口，适配实时交易场景。

### 2.2 工具系统 (`aagt-core/tool.rs`)
工具系统采用高度灵活的 Trait 定义：
*   **Tool Trait**: 要求开发者实现 `name`、`definition` 和 `call`。`definition` 返回标准的 JSON Schema，使代理能准确理解如何传参。
*   **ToolSet**: 一个线程安全的工具库（使用 `DashMap`），支持在 Agent 运行期间动态管理工具。

### 2.3 适配层 (`aagt-providers`)
将不同 LLM 厂商的 API 差异封装在内部：
*   **多模型支持**: 原生支持 OpenAI, Claude, Gemini 以及国产主流模型 DeepSeek。
*   **流式转换**: 将厂商特有的流式协议统一转换为 AAGT 内部的 `StreamingResponse`。

### 2.4 交易增强模块
*   **Risk (风控)**: 专门设计的 `RiskManager`，可以在 Agent 发出交易指令后、实际执行前介入，进行滑点核对、资金限额检查等。
*   **Strategy (策略流水线)**: 将 AI 的决策转化为 `Condition -> Action` 的链式操作，支持自动化定投、抄底等复杂指令。

---

## 三、 核心工作流程 (Reasoning Loop)

1.  **用户输入**: 用户发送交易指令（如 "帮我买入 100 刀的 SOL"）。
2.  **模型推理**: Agent 将指令与工具定义发送给 LLM，LLM 判断需要调用 `swap` 工具。
3.  **工具解析**: Agent 解析 LLM 返回的 `ToolCall` 参数。
4.  **安全评估**: `RiskManager` 对参数进行扫描（例如：买入金额是否超过单笔上限）。
5.  **驱动执行**: 调用具体的 Rust 实现（如 `solana-sdk`）执行链上操作。
6.  **反馈闭环**: 交易结果返回给 LLM，由 LLM 生成最终的自然语言回复。

---

## 四、 核心接口定义 (Rust 参考)

### Tool Trait
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    async fn definition(&self) -> ToolDefinition;
    async fn call(&self, arguments: &str) -> Result<String>;
}
```

### Provider Trait
```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_completion(
        &self,
        model: &str,
        system_prompt: Option<&str>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        // ... 其他参数
    ) -> Result<StreamingResponse>;
}
```

---

## 五、 为什么选择 AAGT？

1.  **高性能**: 基于 Rust 语言，利用 `async/await` 实现高并发，适合实时盯盘机器人。
2.  **交易原生**: 不同于通用的 AI 框架，AAGT 内置了模拟交易（Simulation）、风控（Risk）和策略流水线（Strategy）。
3.  **极简迁移**: 通过 `define_tool!` 宏，开发者可以在几分钟内将现有的 Rust 函数转化为 AI 可调用的工具。

---

## 六、 未来规划

*   **多代理协作 (Swarm)**: 允许研究员 Agent、风控 Agent 和交易员 Agent 协同工作。
*   **长期记忆库**: 集成向量数据库，使 Agent 能够记住用户的交易偏好和历史策略表现。
