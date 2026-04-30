# Project overview

## What Parser is

An AI-powered coding agent that runs in the terminal. Conceptually similar
to Claude Code, but model-agnostic: any OpenAI-compatible endpoint works,
and the user supplies their own API key.

Examples of compatible endpoints: OpenRouter, Ollama, Groq, Together AI,
LM Studio, vLLM, direct OpenAI / Anthropic-compatible proxies. Nothing
about a specific provider is hardcoded in the binary — the endpoint URL,
model name, and API-key environment variable are all read from
`parser.config.toml`.

## The thesis

> Better context management produces better output, regardless of which
> model is used.

Parser does not try to improve the model. It improves **what the model
sees**. That premise drives the four-phase architecture below.

## The four phases

These are conceptual phases of the runtime pipeline, not Rust modules
(the modules will be named after these phases when they're built).

| Phase | Responsibility | Built? |
|---|---|---|
| **1. Indexer** | Reads the codebase, builds AST, dependency graph, and embedding index. Runs in background continuously. | No |
| **2. Retriever** | Given a task, finds the most relevant code chunks using math (semantic similarity + graph traversal + agent history). No LLM involved in selection. | No |
| **3. Assembler** | Builds a structured document from retrieved chunks. Labels, constraints, format instructions. This is what gets sent to the model. | No |
| **4. Compressor** | Maintains a *Decision Log* across the session. Compresses old conversation turns between tasks. Keeps the context window healthy for long sessions. | No |

## The agent system

A single agent today, multi-agent later. The roles defined upfront are:

- **Planner** — breaks a task into steps
- **Coder** — writes and edits code
- **Critic** — reviews proposed changes
- **Debugger** — diagnoses failures
- **Compressor** — compresses old turns into the Decision Log

Each role can use a different model. The user configures which model does
which job in `parser.config.toml` under `[agents]`. If a role is not
specified, it falls back to `model.name`.

The architectural rule we will follow: **define the `Agent` trait now, even
though only `Coder` exists later, so multi-agent can be added without
rewriting existing code.**

## The provider system

Any OpenAI-compatible endpoint works. The architectural rule:

- No hardcoded providers
- User supplies endpoint URL + model name + API-key env-var name
- The config file stores the **endpoint URL**, not a provider name like
  "OpenRouter" — this keeps the binary unaware of which provider is in
  use, which is the whole point of being model-agnostic.

The architectural rule: **define the `Provider` trait now, even though
only one OpenAI-compatible implementation will exist initially.**

## The 10-step roadmap

| Step | Deliverable | Status |
|---|---|---|
| 1 | Config system + CLI scaffold + release binary | ✅ done |
| 2 | Provider trait + OpenAI-compatible provider | next |
| 3 | Agent trait + Coder agent | |
| 4 | Indexer (AST + dep graph + embeddings) | |
| 5 | Retriever (semantic + graph + history) | |
| 6 | Assembler (structured prompt builder) | |
| 7 | Compressor + Decision Log | |
| 8 | Multi-agent (Planner/Critic/Debugger) | |
| 9 | Tool-use plumbing (file edits, shell, etc.) | |
| 10 | Polish + packaging + cross-platform release builds | |

Steps 2-10 are intentionally not designed in detail yet. The architectural
rules below ensure they can be added without rewriting step 1.

## Architectural rules followed throughout

These hold for every step, not just step 1:

1. **Cross-platform paths via `PathBuf`, never strings.**
2. **Nothing hardcoded. Everything in config** — endpoint, model, paths,
   per-agent model overrides.
3. **Clear error messages that tell the user exactly what to do.** Not "config
   error" but "no config found at `C:\Users\...\.parser\parser.config.toml`
   — run `parser init` to create one".
4. **`Agent` trait defined now even though only `Coder` exists yet.**
5. **`Provider` trait defined now even though only one provider exists yet.**
6. **Multi-agent must be addable later without rewriting existing code.**

## Where the code is today

```
src/main.rs             CLI: init / run / external-subcommand fallback
src/config/mod.rs       Config struct, loader, `parser init` wizard, 2 tests
.cargo/config.toml      gnullvm linker + ar + +crt-static rustflag
Cargo.toml              5 runtime deps (serde, toml, clap, dirs, url) + tempfile dev-dep
```

Both runtime invariants the user explicitly asked for are verified by
unit tests:

1. `Config::load()` returns the **resolved API key value** in
   `model.api_key`, not the env-var name.
2. `Config::load()` returns a **fully-expanded absolute `PathBuf`** in
   `paths.data_dir`, with `~` resolved to the real home directory.

See [05-testing.md](documentation/05-testing.md) for the assertions that prove this.
