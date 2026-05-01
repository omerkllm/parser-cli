# Developers

This page is for people who want to understand or extend
Parser's source code. It walks through the file structure,
the planned architecture, the two main traits, and how the
config loader works.

For per-file deep dives, see the
[`documentation/`](../documentation/) folder at the project
root — one Markdown file per source file with line-numbered
references.

## File structure

```
parser-cli/
├── Cargo.toml                package metadata, deps, [profile.release]
├── Cargo.lock                pinned dep versions (committed)
├── .cargo/config.toml        local linker + INCLUDE/LIB paths (gitignored)
├── .github/workflows/ci.yml  GitHub Actions: tests + clippy + release
├── .gitignore
├── src/
│   ├── main.rs               CLI entry point and subcommand routing
│   ├── config/mod.rs         Config schema, loader, init wizard, tests
│   ├── agents/mod.rs         Agent trait + CoderAgent placeholder + tests
│   └── providers/mod.rs      ModelProvider trait + Message + ProviderError
├── target/                   build output (gitignored)
├── documentation/            per-file technical docs (developer-focused)
└── guide/                    user/developer plain-English guide (you are here)
```

One-line summary of each source file:

| File | Purpose |
|---|---|
| `src/main.rs` | Parses command-line arguments via `clap`, routes to `init` / `run` / free-form, drives the tokio runtime, prints errors. |
| `src/config/mod.rs` | Owns the TOML schema (`[model]`, `[parameters]`, `[paths]`, `[agents]`), the two-layer loader, every validation rule, the `parser init` wizard, and 14 unit tests. |
| `src/agents/mod.rs` | Defines the `Agent` trait shared by every reasoning role, the `AgentInput` / `AgentOutput` / `AgentError` types, and a real `CoderAgent` that streams through a provider and accumulates the response. Has 2 unit tests. |
| `src/providers/mod.rs` | Defines the `ModelProvider` trait that talks to an OpenAI-compatible endpoint, the `Message` and `Role` wire types, and `ProviderError`. Provides `OpenAIProvider`, the real HTTP+SSE implementation. Has 4 unit tests using `wiremock`. |

## The four pipeline phases

The architectural thesis behind Parser is that **better context
management produces better output, regardless of which model is
used.** The pipeline that achieves this has four planned phases.
Steps 4-7 of the project roadmap each build one phase. None of
them exist in code yet.

| Phase | What it will do |
|---|---|
| **1. Indexer** | Walks the workspace, parses every source file into an AST, builds a dependency graph, and computes embeddings for semantic search. The output is a queryable index of "what's in this codebase". |
| **2. Retriever** | Given a task, picks the most relevant chunks from the index — combining semantic similarity, graph reachability (callers/callees), and conversation history. The output is a candidate set of context fragments. |
| **3. Assembler** | Takes the candidate set and the task, builds a structured prompt that fits the model's context window. Decides what goes in the system message vs. the user turn, what gets summarized, what gets truncated. The output is a finished `Vec<Message>` ready for the provider. |
| **4. Compressor** | When a conversation grows past the context limit, summarizes older turns into a Decision Log — terse records of what was decided and why. Future turns reference the log instead of replaying full history. |

Step 3 of the roadmap (one step before any of these are built)
is a measurement step: run the baseline agent against a fixed
task set, see where it fails, and confirm the four phases above
are worth their cost before writing them.

## The five planned agents

Parser will host five agents, each potentially backed by a
different model. Today only the `CoderAgent` placeholder exists.

| Agent | Role |
|---|---|
| **Planner** | Breaks a task into ordered steps. The first agent invoked for any non-trivial request. |
| **Coder** | Writes and edits code. The default agent for direct coding tasks. |
| **Critic** | Reviews proposed changes. Catches obvious mistakes the Coder missed. |
| **Debugger** | Investigates failures: reads stack traces, suggests root causes, proposes fixes. |
| **Compressor** | Summarizes old conversation turns into Decision Log entries. Runs in the background, not invoked directly. |

All five share the `Agent` trait shape (see below), so adding
one is a single `impl Agent for ...` block. There's no registry
or factory — the caller picks which agent to instantiate based
on the task.

## The `Agent` trait

```rust
pub trait Agent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError>;
}
```

Three things to know:

1. **Native `async fn` in trait, no `#[async_trait]` macro.**
   Stable since Rust 1.75. Used here because every call site
   knows the concrete agent type at compile time
   (e.g. `CoderAgent::run`) — there's no `&dyn Agent` need
   today, so we don't pay for the boxed-future cost the macro
   would impose.
2. **`AgentInput` is moved in, `AgentOutput` is moved out.**
   Owned in/owned out is the right shape for an agent that
   may rewrite its input (adding a system prompt, appending a
   tool result) before forwarding to the provider.
3. **`provider` is borrowed as `&dyn ModelProvider`.** The
   agent doesn't own the provider — it borrows for the
   duration of the call. The same provider instance can be
   passed to multiple agents in sequence.

### Adding a new agent

Mechanical pattern:

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
        // 2. Call `provider.complete(messages).await?`.
        //    (The `?` propagates ProviderError as
        //    AgentError::ProviderFailed via the From impl.)
        // 3. Parse the response into a structured output.
        // 4. Wrap in AgentOutput and return.
    }
}
```

No registry, no `dyn Agent`, no factory. The caller in
`main.rs` chooses which agent to construct based on the task.

## The `ModelProvider` trait

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(
        &self,
        messages: Vec<Message>,
    ) -> Result<String, ProviderError>;

    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>,
        ProviderError,
    > {
        let response = self.complete(messages).await?;
        Ok(Box::pin(futures::stream::once(async move { Ok(response) })))
    }
}
```

Two methods, one required:

- **`complete`** is the only required method. Returns the full
  response as a `String`. Convenient for tests, batch jobs, or
  any caller that doesn't need streaming.
- **`stream_completion`** has a default implementation that
  wraps `complete` in a single-item stream. The real streaming
  provider (`OpenAIProvider`) overrides it to parse SSE chunks
  as they arrive.

The trait yields `Result<String, ProviderError>` items rather
than bare `String` so mid-stream failures (a malformed SSE
chunk, a dropped TCP connection) can be reported to the caller
instead of silently ending the stream.

### Caller owns the system message

The provider serializes whatever `Vec<Message>` it receives,
nothing implicit. Both `main.rs::run_task` and
`CoderAgent::run` prepend a `Role::System` turn before calling
the provider.

### `OpenAIProvider` — the real implementation

```rust
pub struct OpenAIProvider {
    endpoint: String,
    model: String,
    api_key: String,
    max_tokens: u32,
    temperature: f32,
    client: reqwest::Client,
}
```

Built once at startup via `OpenAIProvider::from_config(&cfg)`.
Works with any endpoint that speaks the OpenAI chat-completions
wire format: OpenRouter, OpenAI, Groq, Together AI, Ollama, LM
Studio, etc. See [providers.md](../documentation/providers.md)
for the full SSE flow, header set, and error mapping.

### The streaming end-to-end flow

The interactive CLI path:

1. `main.rs::run_task` validates the trimmed task and loads
   `Config`.
2. Builds `OpenAIProvider::from_config(&cfg)`.
3. Builds messages: `[Role::System, Role::User]`.
4. Calls `provider.stream_completion(messages).await?`. A
   pre-stream failure (auth / network / API status) propagates
   here; nothing has printed yet.
5. Prints `User: <task>` + divider + `Response: `.
6. Loops `while let Some(item) = stream.next().await`:
   - `Ok(chunk)` → `print!("{}", chunk)`, flush stdout, append
     to a `collected` buffer.
   - `Err(e)` → newline, `eprintln!("error: {}", e)`, plus
     `Stream interrupted. Partial response shown above.` if
     `collected` is non-empty. Exit 1.
7. After clean stream end: newline + closing divider. If
   `collected.is_empty()` → print empty-response message and
   exit 1.

The agent layer (`CoderAgent::run`) follows the same shape
internally but *accumulates* chunks into a single `String`
before returning, and is bypassed by `main.rs` because the
collect-then-return pattern is incompatible with live
token-by-token output.

## How config loading works

Parser uses a **two-layer loading strategy**, deliberately split
so the on-disk shape and the runtime shape are different types.

```
                        on-disk TOML
                             │
                  toml::from_str   ┐
                             ▼     │
                       RawConfig   │  Layer 1: permissive shape
                             │     │  every field Option<T>
                  Config::from_raw │
                             ▼     ┘
                          Config      Layer 2: validated, resolved
                                      every field guaranteed present
```

### Layer 1 — `RawConfig`

A struct whose fields are all `Option<T>`. Mirrors the on-disk
TOML structure exactly. Its only job is to deserialize cleanly
even when the file is missing optional sections. It does no
validation.

### Layer 2 — `Config::from_raw`

Converts `RawConfig` into the strict `Config` used by the rest
of the program. Five things happen here:

1. **Required fields are checked.** Anything `None` → `MissingField` error.
2. **Defaults are applied.** `max_tokens = 4096`, `temperature = 0.7`,
   per-agent model name = the global model name, etc.
3. **Paths are expanded.** `~/...` becomes an absolute `PathBuf`
   on every platform.
4. **The endpoint is validated and normalized.** Schema check
   plus trailing-slash and trailing-`/chat/completions` stripping.
5. **The API key is resolved.** The env var named in
   `api_key_env` is read; the *value* gets stored in
   `model.api_key` so consumers don't have to call
   `std::env::var` themselves.

After `Config::from_raw` returns `Ok`, every field on `Config`
is guaranteed present, validated, and ready to use. **No further
validation runs anywhere in the codebase.** The rest of the
program treats `Config` as a single source of truth and reads
fields directly without re-checking.

### Why the split

Three benefits:

- **Type-driven validation.** `Config` doesn't have any `Option`
  on required fields, so consumers can't forget to handle the
  missing case.
- **Single error path.** Every validation error funnels through
  `ConfigError`, with a hand-written `Display` per variant —
  every error message tells the user exactly what to fix.
- **Cheap to extend.** Adding a new field is a `Raw…` entry, a
  resolved-struct entry, and a `from_raw` line. No need to
  thread `Option`s through the rest of the codebase.

## Where things stand

**Step 1 is complete.** Config system, CLI scaffold, the two
trait shapes, release profile, CI, input validation.

**Step 2 is complete.** This step replaced the placeholders
with real implementations:

- `OpenAIProvider` — real HTTP+SSE against any
  OpenAI-compatible endpoint. Uses `reqwest` with rustls for
  the client, hand-rolled SSE line parsing for streaming.
- `CoderAgent::run` — builds the message list, opens a
  streaming completion, accumulates chunks, returns the full
  response. Bypassed by the CLI in favour of direct streaming.
- `main.rs::run_task` — the streaming CLI path. Tokens print
  to the terminal as they arrive.
- `ModelProvider::stream_completion` returns
  `Stream<Item = Result<String, ProviderError>>` (not bare
  `String`) so mid-stream errors can reach the caller.

The release binary is now about 2.7 MB. The trait shapes are
unchanged compared to Step 1 *with the single exception of the
stream item type*; the rest of the surface remained stable
through the change.

**Step 3** then runs the real Step-2 agent against a fixed task
set to measure baseline output quality before the four
context-management phases (Steps 4-7) get built. Context
infrastructure is expensive; Step 3 is the checkpoint that
proves it's worth building.

## Further reading

- [`documentation/`](../documentation/) — per-file technical
  docs with line-numbered references into the source.
- [`documentation/main.md`](../documentation/main.md) —
  detailed walkthrough of `src/main.rs`.
- [`documentation/config.md`](../documentation/config.md) —
  every detail of the config loader.
- [`documentation/agents.md`](../documentation/agents.md) —
  the `Agent` trait in depth.
- [`documentation/providers.md`](../documentation/providers.md) —
  the `ModelProvider` trait in depth.
- [`documentation/04-toolchain.md`](../documentation/04-toolchain.md) —
  build system, deps, MSVC toolchain setup.
- [`documentation/testing.md`](../documentation/testing.md) —
  test infrastructure, how to add tests, smoke tests.
