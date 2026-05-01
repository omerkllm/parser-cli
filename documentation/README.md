# Parser — developer documentation

Parser is a model-agnostic AI coding agent that runs in the terminal.
The user supplies their own API key and any OpenAI-compatible endpoint
(OpenRouter, Ollama, Groq, Together AI, LM Studio, etc.). The binary
hardcodes nothing about which provider is on the other end.

The project's central thesis: **better context management produces
better output, regardless of which model is used.** Parser does not
try to improve the model. It improves what the model sees.

This folder is the developer's reference. Each file in it covers one
piece of the codebase in detail — what it does, how it's wired, why
it's shaped the way it is, and how to extend it.

## Quickstart for new contributors

```powershell
# 1. Build and run (assumes Rust + VS Build Tools set up — see 04-toolchain.md).
cd D:\parser-cli
cargo build --release

# 2. Configure (one-time wizard creates ~/.parser/parser.config.toml).
.\target\release\parser.exe init

# 3. Set the env var your config points at, then run a task.
$env:OPENROUTER_API_KEY = "sk-or-v1-..."
.\target\release\parser.exe run "fix the jwt bug"

# 4. Run the test suite.
cargo test --bin parser config::tests
```

## File index

One doc per source file plus three cross-cutting docs (build, testing,
this README).

| Doc | Covers |
|---|---|
| [main.md](main.md) | `src/main.rs` — CLI entry point, subcommand routing, async runtime wiring. |
| [config.md](config.md) | `src/config/mod.rs` — `Config` struct, two-layer loader, `parser init` wizard, validation rules. |
| [agents.md](agents.md) | `src/agents/mod.rs` — `Agent` trait, `AgentInput` / `AgentOutput` / `AgentError`, `CoderAgent` placeholder. |
| [providers.md](providers.md) | `src/providers/mod.rs` — `ModelProvider` trait, `Message`, `ProviderError`, `NoopProvider` stub. |
| [04-toolchain.md](04-toolchain.md) | `Cargo.toml`, `.cargo/config.toml`, `[profile.release]` flags, the MSVC + VS Build Tools 2022 toolchain, reproducing the setup on a fresh Windows machine. |
| [testing.md](testing.md) | Test infrastructure, the two existing invariant tests, how to add new ones, manual smoke tests. |

## Project status

The codebase has the config system, CLI scaffold, async runtime, the
`Agent` + `ModelProvider` trait shapes, the release-profile flags,
and CI. The real provider implementation, real Coder agent, and the
context-management phases (Indexer / Retriever / Assembler /
Compressor) are not built yet.

| Step | Deliverable | Status |
|---|---|---|
| 1 | Config + CLI + CI + release profile | done |
| 2 | Real provider + real Coder agent end-to-end | next |
| 3 | Measure output quality without context management | |
| 4 | Indexer (AST + dep graph + embeddings) | |
| 5 | Retriever (semantic + graph + history) | |
| 6 | Assembler (structured prompt builder) | |
| 7 | Compressor + Decision Log | |
| 8 | Multi-agent (Planner / Critic / Debugger) | |
| 9 | Polish + cross-platform release builds | |

### Why Step 3 is a measurement step, not a build step

Context management infrastructure — the indexer, retriever, assembler,
compressor — is expensive to design and expensive to maintain. It only
earns that cost if the baseline agent (Step 2: a plain provider call,
no retrieval, no compression) has measurable output-quality problems
on real coding tasks. Step 3 is the checkpoint that runs that
baseline against a fixed task set and records where it succeeds and
where it fails. The result either validates Steps 4-7 (the four
phases of context management) or invalidates them and forces a
redesign before any of that code is written. Building Steps 4-7
without Step 3's evidence would be guessing.

Steps 2-9 are intentionally not designed in detail. The trait
boundaries from Step 1 give them room to land without rewriting
existing code.

## Continuous integration

GitHub Actions runs on every push and pull request to `main`. The
workflow lives in [.github/workflows/ci.yml](.github/workflows/ci.yml)
and does three things:

| Job | Runner(s) | Checks |
|---|---|---|
| `test` | ubuntu-latest, windows-latest, macos-latest | `cargo test --bin parser` on all three OSes. |
| `lint` | ubuntu-latest | `cargo fmt --check` and `cargo clippy -- -D warnings`. |
| `release` | windows-latest, macos-latest, ubuntu-latest | Triggered only on tags matching `v*`. Builds release binaries for `x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-unknown-linux-musl` (the musl build installs `musl-tools` first), then uploads each as a GitHub release asset via `softprops/action-gh-release@v2`. |

Toolchain installs use `dtolnay/rust-toolchain@stable`; build caching
uses `Swatinem/rust-cache@v2`. The Windows CI runner uses
`x86_64-pc-windows-msvc` with a properly registered Visual Studio
install — it does **not** depend on the local `.cargo/config.toml`
workaround documented in [04-toolchain.md](04-toolchain.md), because
that file is gitignored and never reaches the runner.

To cut a release: tag the commit with `vX.Y.Z` and push the tag. The
release job builds all four binaries and attaches them to the
auto-created GitHub release.

## Architectural rules followed throughout

These hold for every step, not just current work:

1. **Cross-platform paths via `PathBuf`, never strings.**
2. **Nothing hardcoded.** Endpoint, model, paths, per-agent model
   overrides — all in `parser.config.toml`.
3. **Clear error messages that tell the user exactly what to do.**
   Not "config error" but "no config found at `…\.parser\parser.config.toml`
   — run `parser init` to create one".
4. **`Agent` trait defined now even though only `Coder` exists yet.**
5. **`ModelProvider` trait defined now even though no real provider
   exists yet.**
6. **Multi-agent must be addable later without rewriting existing code.**

## Repository layout

```
D:/parser-cli/
├── Cargo.toml             package, deps, [profile.release]
├── Cargo.lock             pinned dep versions (committed)
├── .cargo/config.toml     MSVC linker path + INCLUDE/LIB env + +crt-static rustflag (gitignored)
├── .gitignore
├── src/
│   ├── main.rs            CLI entry + subcommand routing (async via tokio)
│   ├── config/mod.rs      Config struct, loader, init wizard, 2 unit tests
│   ├── agents/mod.rs      Agent trait + CoderAgent placeholder
│   └── providers/mod.rs   ModelProvider trait + Message + ProviderError + NoopProvider stub
├── target/                build output (gitignored, ~1.14 GB after release build)
└── documentation/         you are here
```

External state on this machine that the project depends on:

```
~/.cargo/bin/                                                       cargo, rustc, rustup
~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/                 Rust MSVC toolchain (active)
C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/     MSVC compiler, link.exe, static CRT libs
C:/Program Files (x86)/Windows Kits/10/                             Windows 11 SDK 10.0.26100 (headers + import libs)
~/.parser/parser.config.toml                                        the user's real Parser config
```

See [04-toolchain.md](04-toolchain.md) for how to install or reproduce these.

## Current binary metrics

After the release-profile optimizations landed (`opt-level=3`, `lto`,
`codegen-units=1`, `strip`, `panic="abort"`):

| Metric | Value |
|---|---|
| `target/release/parser.exe` size | **0.99 MB** |
| Peak working-set RAM on `parser run hello` | **9.66 MB** |
| Cold release build | ~45 s |
| Incremental release build | <2 s |

The release-profile flags (LTO, single codegen-unit, strip,
panic=abort) cut the binary down from ~1.7 MB unoptimized. See
[04-toolchain.md](04-toolchain.md#release-profile) for what each
flag does.
