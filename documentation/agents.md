# `src/agents/mod.rs` — Agent trait + CoderAgent placeholder

50 lines. Defines the shape every reasoning role in Parser shares.
Today there is one role with a placeholder body (`CoderAgent`); four
more roles will land later as separate `impl Agent` blocks against
the same trait.

## Roles planned

Five agents are conceptually planned in the project's architecture:

| Role | Job |
|---|---|
| **Planner** | Breaks a task into steps. |
| **Coder** | Writes and edits code. |
| **Critic** | Reviews proposed changes. |
| **Debugger** | Diagnoses failures. |
| **Compressor** | Compresses old conversation turns into the Decision Log. |

Each role can use a different model — the user configures which model
does which job under `[agents]` in `parser.config.toml`. See
[config.md](config.md) for the schema.

The architectural rule: **define the `Agent` trait now even though
only `Coder` exists yet**, so multi-agent gets added later without
rewriting existing code.

## The trait

[src/agents/mod.rs:23](src/agents/mod.rs:23):

```rust
pub trait Agent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError>;
}
```

Three things to notice:

### 1. Native `async fn` in trait — no macro

The trait does not use `#[async_trait]`. Native `async fn` in traits
has been stable in Rust since 1.75. We can use it here because
nothing dispatches `Agent` through `dyn` — the caller in
[main.rs:57](src/main.rs:57) instantiates `CoderAgent` directly,
calling `agent.run(...)` via static dispatch.

The trade-off is that `dyn Agent` would not work today (the compiler
rejects it without explicit `Box<dyn Future>` boxing). If a future
step wants polymorphic agents in a collection, switch to
`#[async_trait]` then — not now.

[providers.md](providers.md) makes the opposite choice: it uses
`#[async_trait]` because `&dyn ModelProvider` is required by the
agent's signature.

### 2. `&dyn ModelProvider` — borrowed, not owned

The agent does not take ownership of the provider. It borrows one
for the duration of the call. The lifetime is implicit — the borrow
lives only until `run` returns — so the agent cannot accidentally
hold a provider across calls.

This also means the same provider instance can be passed to multiple
agents in sequence without cloning.

### 3. `AgentInput` taken by value, `AgentOutput` returned by value

Owned in/owned out is the right shape for an agent that may rewrite
or extend its input (e.g., adding a system prompt, appending a tool
result) before forwarding to the provider. Borrowing in would force
a clone before mutation.

## `AgentInput`

[src/agents/mod.rs:3](src/agents/mod.rs:3):

```rust
pub struct AgentInput {
    pub task: String,
    pub conversation_history: Vec<Message>,
}
```

Fields:

- **`task`** — the user-supplied request, exactly as typed. For
  one-shot CLI invocations this is the only meaningful field.
- **`conversation_history`** — prior turns of an ongoing session.
  Empty for one-shot CLI runs. Populated when an agent is part of
  a longer flow (e.g., the Compressor reading old turns to compress,
  or a Planner referencing earlier decisions).

`Message` is imported from `providers::` rather than redefined here.
See [providers.md](providers.md#message) for the canonical shape.
The same struct is used wherever role/content pairs travel — wire
format, agent input, decision log, etc.

## `AgentOutput`

[src/agents/mod.rs:8](src/agents/mod.rs:8):

```rust
pub struct AgentOutput {
    pub response: String,
}
```

A single string for now. Step 4+ will likely add structured fields
(tool calls, planned subtasks, decision-log entries). Adding fields
is non-breaking; the shape is intentionally minimal today so the
placeholder doesn't pretend to know what real output looks like.

## `AgentError`

```rust
#[derive(Debug)]
pub enum AgentError {
    ProviderFailed(ProviderError),
    InvalidResponse(String),
    ContextLimitExceeded,
    TaskEmpty,
    TaskTooLong { length: usize, max: usize },
}
```

The five failure modes an agent run can surface:

| Variant | When it fires |
|---|---|
| `ProviderFailed(ProviderError)` | The underlying [`ModelProvider`](providers.md#the-modelprovider-trait) returned an error during completion — network failure, non-2xx response, auth rejection, malformed stream. The original [`ProviderError`](providers.md#providererror) is wrapped, not flattened, so the network/API/auth/stream context isn't lost. |
| `InvalidResponse(String)` | The completion succeeded at the transport level but produced a body the agent couldn't use: malformed JSON in a structured-output mode, an empty body where content was required, a tool-call payload that didn't match the schema. The string describes what was wrong. |
| `ContextLimitExceeded` | The conversation history plus the new task would exceed the configured model's context window. The agent declined to send a request guaranteed to be truncated, leaving the caller to drop history or shorten the task. Carries no payload — the recovery is the same in every case. |
| `TaskEmpty` | The task string was empty after trimming. Surfaced before any provider call, so an empty prompt never leaves the binary. The check is `input.task.trim().is_empty()`, so `"   \t  "` (only whitespace) fails the same as `""`. |
| `TaskTooLong { length, max }` | The task string (after trimming) exceeded `MAX_TASK_LEN` (currently 32,768 characters). Carries the actual `length` and the configured `max` so the error message tells the user exactly how much they need to trim. |

`Display` writes them as:

| Variant | Message |
|---|---|
| `ProviderFailed(e)` | `"provider error: {e}"` (delegates to `ProviderError`'s own `Display`) |
| `InvalidResponse(s)` | `"agent received invalid response: {s}"` |
| `ContextLimitExceeded` | `"context limit exceeded"` |
| `TaskEmpty` | `"task cannot be empty"` |
| `TaskTooLong { length, max }` | `"task is {length} characters, maximum is {max}"` |

`impl std::error::Error for AgentError {}` lets the type widen
into `Box<dyn Error>` at [main.rs](src/main.rs) where it joins
`ConfigError` and `ProviderError` under one return type.

### `From<ProviderError>` conversion

```rust
impl From<ProviderError> for AgentError {
    fn from(e: ProviderError) -> Self {
        AgentError::ProviderFailed(e)
    }
}
```

This is what makes provider calls inside `run` ergonomic. With the
`From` impl in place, any `?` on a method that returns
`Result<_, ProviderError>` propagates as
`Result<_, AgentError>` automatically — no manual `map_err`. So
the real Coder body in the next step can write:

```rust
let response = provider.complete(messages).await?;  // ProviderError → AgentError::ProviderFailed
```

…rather than threading the error type by hand.

## `CoderAgent` — the placeholder

[src/agents/mod.rs:31](src/agents/mod.rs:31):

```rust
pub struct CoderAgent;

impl CoderAgent {
    pub fn new() -> Self {
        CoderAgent
    }
}

impl Agent for CoderAgent {
    async fn run(
        &self,
        input: AgentInput,
        _provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError> {
        let trimmed = input.task.trim();
        if trimmed.is_empty() {
            return Err(AgentError::TaskEmpty);
        }
        if trimmed.len() > MAX_TASK_LEN {
            return Err(AgentError::TaskTooLong {
                length: trimmed.len(),
                max: MAX_TASK_LEN,
            });
        }
        Ok(AgentOutput {
            response: "Coder agent placeholder".to_string(),
        })
    }
}
```

The body has two pieces of real logic, both input validation:

1. **Empty-task guard.** `input.task.trim()` strips leading and
   trailing whitespace; if the remainder is empty the run
   short-circuits with [`AgentError::TaskEmpty`](#agenterror).
   This means `"    "` fails the same as `""` — the check looks
   at semantic content, not raw character count.
2. **Length cap.** If the trimmed task exceeds `MAX_TASK_LEN`
   (32,768 characters), the run short-circuits with
   [`AgentError::TaskTooLong`](#agenterror) carrying the actual
   length and the max. Catches pathological inputs (a multi-MB
   prompt accidentally piped in) before they ever reach the
   provider's request body.

Everything else is still placeholder: the `_provider`
underscore-prefix suppresses the unused-argument warning until
the real body wires it in, and the success path returns the
literal `"Coder agent placeholder"` regardless of what
`input.task` contains.

## Tests

Two unit tests in `#[cfg(test)] mod tests`, both using
`#[tokio::test]` so they can `.await` `CoderAgent::run`:

| Test | Proves |
|---|---|
| `whitespace_only_task_returns_task_empty` | A task of just spaces/tabs/newlines fails the same as `""`, returning `AgentError::TaskEmpty`. |
| `task_longer_than_max_returns_task_too_long` | A task longer than `MAX_TASK_LEN` after trimming returns `AgentError::TaskTooLong { length, max }` carrying the actual lengths so the user sees how much to trim. |

Both tests use `NoopProvider` for the `&dyn ModelProvider` slot;
the validation runs before any provider call, so the noop never
gets invoked.

The unit-struct shape (`pub struct CoderAgent;` — no fields) is
right for a stateless placeholder. Real `CoderAgent` will likely
carry configuration: model name override, temperature, system prompt
template, retry policy. Those fields get added when they earn their
keep.

## How the agent is wired into main

From [main.rs:57](src/main.rs:57):

```rust
let agent = CoderAgent::new();
let provider = NoopProvider;
let input = AgentInput {
    task: task.to_string(),
    conversation_history: Vec::new(),
};
let output = agent.run(input, &provider).await?;
println!("{}", output.response);
```

`NoopProvider` is the throwaway stub that satisfies the
`&dyn ModelProvider` slot until Step 3's real OpenAI-compatible
provider replaces it. See
[providers.md](providers.md#noopprovider).

## Adding a new agent

The pattern is mechanical:

```rust
pub struct PlannerAgent;

impl PlannerAgent {
    pub fn new() -> Self { PlannerAgent }
}

impl Agent for PlannerAgent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError> {
        // 1. Build planner-specific messages from `input`.
        // 2. Call `provider.stream_completion(messages).await`.
        // 3. Collect chunks from the stream into a single String.
        // 4. Wrap in AgentOutput and return.
    }
}
```

No registry, no factory, no `dyn Agent`. The caller picks which
agent to instantiate based on the task. Multi-agent orchestration
(a Planner that decides which agent to delegate to) is its own step
on the roadmap; until then, agents are constructed by name in code.

## What this module deliberately doesn't do

- **No agent registry or service-locator.** The list of agents lives
  in source code, not in config.
- **No `dyn Agent`.** `CoderAgent` is statically dispatched. If a
  future use case wants a polymorphic collection of agents, switch
  to `#[async_trait]` and `Box<dyn Agent>` then.
- **No streaming back from the agent.** The `Stream` lives at the
  provider layer; the agent collects chunks and returns the
  assembled response. Step 4+ may revisit this if streaming all the
  way up to the CLI becomes useful.
- **No retry logic, no logging, no telemetry.** All future concerns,
  added when there's a use case.
- **No tool-use.** The signature has no `Vec<Tool>` argument yet.
  Step 4 will likely extend the shape.

## Cross-references

- [providers.md](providers.md) — the trait the agent's second
  argument refers to, plus the canonical `Message` shape.
- [main.md](main.md) — where `CoderAgent::run` gets called.
- [config.md](config.md) — the `[agents]` section that future agents
  will read for their per-role model overrides.
- [testing.md](testing.md) — currently no tests for this module
  (placeholder code); pattern for adding them.
