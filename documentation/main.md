# `src/main.rs` — CLI entry point

68 lines. The binary's outermost layer: argument parsing, subcommand
routing, async runtime, error printing. Almost all the real logic lives
in submodules; this file is glue.

## Responsibilities

1. Parse command-line arguments via `clap`.
2. Dispatch to one of three handlers: `init`, `run`, or free-form.
3. Drive the tokio runtime so async handlers can `.await`.
4. Print errors and set the process exit code.

## Module declarations

[src/main.rs:3](src/main.rs:3):

```rust
mod agents;
mod config;
mod providers;
```

Three submodules, each living in `src/<name>/mod.rs`. Loaded in
alphabetical order; the order itself doesn't matter to the compiler.

## CLI structure (clap derive)

[src/main.rs:10](src/main.rs:10):

```rust
#[derive(Parser)]
#[command(name = "parser")]
#[command(about = "AI-powered coding agent that runs in the terminal", long_about = None)]
#[command(version)]
#[command(arg_required_else_help = true)]
#[command(allow_external_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
```

Each `#[command(...)]` attribute does one job:

| Attribute | Effect |
|---|---|
| `name = "parser"` | Program name in help text and error messages. |
| `about = "..."` | One-line description shown in `--help`. |
| `version` | Generates `--version` from `Cargo.toml`'s version. |
| `arg_required_else_help = true` | Bare `parser` (no args) prints help and exits with code 2 instead of running. |
| `allow_external_subcommands = true` | Unknown first words don't error; they fall through to the `External` arm of `Commands`. This is what makes `parser "fix the jwt bug"` work. |

## The three commands

[src/main.rs:21](src/main.rs:21):

```rust
#[derive(Subcommand)]
enum Commands {
    /// Create a parser config file by answering 3 questions
    Init,

    /// Run a coding task: `parser run "fix the jwt bug"`
    Run {
        #[arg(required = true, num_args = 1..)]
        task: Vec<String>,
    },

    /// Free-form task: `parser "fix the jwt bug"`
    #[command(external_subcommand)]
    External(Vec<String>),
}
```

- **`Init`** — no args. Calls [`config::init()`](config.md#init-wizard).
- **`Run { task }`** — `num_args = 1..` means one or more words; clap
  collects them into a `Vec<String>` which we re-join with a space.
  The `required = true` ensures `parser run` (with no task) errors
  with a usage message rather than running an empty task.
- **`External(Vec<String>)`** — captures any unknown first word plus
  everything after it. Combined with `allow_external_subcommands` on
  `Cli`, this is what makes the free-form path work.

The doc comments (`///`) become the per-subcommand `--help` text.

## Why three command shapes for two semantics

`Run { task }` and `External(words)` both end up at `run_task(...)`.
The split exists because clap can't otherwise distinguish "the user
typed `run` as the subcommand" from "the user typed an unknown
subcommand we should treat as a task." Without the explicit `Run`
arm, `parser run "fix"` would land in `External(["run", "fix"])` and
the word "run" would become part of the task — wrong.

## Async runtime

[src/main.rs:37](src/main.rs:37):

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() {
    ...
}
```

`#[tokio::main]` is a macro that:

1. Wraps `main`'s body in an async block.
2. Builds a tokio runtime.
3. Calls `Runtime::block_on(...)` on that block.

The default flavor is `multi_thread` (work-stealing). It needs the
`rt-multi-thread` Cargo feature, which we deliberately don't enable
to keep the binary small. Adding `flavor = "current_thread"` pins
the macro to the single-threaded runtime, which only requires the
`rt` feature already in `Cargo.toml`.

For a CLI doing one task at a time, work-stealing has no upside —
the tokio current_thread runtime is enough to drive `.await` on
provider I/O. If a future step (e.g., the indexer running parallel
tasks) needs concurrency, swap the feature to `rt-multi-thread` and
remove the `flavor` attribute.

## Error handling

[src/main.rs:40](src/main.rs:40):

```rust
let result: Result<(), Box<dyn std::error::Error>> = match cli.command {
    Commands::Init => config::init().map_err(Into::into),
    Commands::Run { task } => run_task(&task.join(" ")).await,
    Commands::External(words) => run_task(&words.join(" ")).await,
};
if let Err(e) = result {
    eprintln!("error: {}", e);
    std::process::exit(1);
}
```

`Box<dyn std::error::Error>` unifies three concrete error types:
`config::ConfigError`, `agents::AgentError`, and any future error
type that `std::error::Error`-implements. The `?` operator inside
`run_task` widens each into the box automatically.

The `Init` arm uses `.map_err(Into::into)` because `config::init()`
returns `Result<(), ConfigError>` directly without going through `?`,
so we widen explicitly.

On error, the message is printed to stderr (`eprintln!`) and the
process exits with status `1`. Successful runs return without an
explicit exit (default exit code `0`). Help/version paths exit with
code `2` from inside clap.

## `run_task`

[src/main.rs:51](src/main.rs:51):

```rust
async fn run_task(task: &str) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = config::Config::load()?;
    println!("Config loaded successfully");
    println!("Model: {}", cfg.model.name);
    println!("Endpoint: {}", cfg.model.endpoint);

    let agent = CoderAgent::new();
    let provider = NoopProvider;
    let input = AgentInput {
        task: task.to_string(),
        conversation_history: Vec::new(),
    };
    let output = agent.run(input, &provider).await?;
    println!("{}", output.response);

    Ok(())
}
```

Five steps:

1. Load and validate config via `Config::load()`. Any failure
   short-circuits with `?` and propagates up to `main`.
2. Print confirmation lines so the user sees that config loaded and
   which model/endpoint will be used.
3. Build the agent input — task string plus an empty conversation
   history (no prior turns in a one-shot CLI invocation).
4. Construct a `CoderAgent` and a `NoopProvider`, then await
   `agent.run(input, &provider)`. The provider is borrowed; the
   input is moved.
5. Print the agent's response.

`NoopProvider` is a temporary stub. Step 2 deletes it and substitutes
the real OpenAI-compatible implementation — no other change to this
function will be needed since the call site is already
trait-dispatched.

## Exit codes

| Code | Cause |
|---|---|
| `0` | Successful run (any subcommand). |
| `1` | Any error returned from a handler — config load failure, agent error, etc. |
| `2` | clap-side error: bare `parser` invocation, unknown flag, malformed args, `--help`, `--version`. |

## How to add a new subcommand

1. Add a variant to `Commands` with a doc comment.
2. Add an arm in `main`'s `match cli.command { ... }` that dispatches
   to a handler function.
3. Write the handler. If it does I/O, make it `async fn` and `.await`
   it from the match arm.
4. The handler returns `Result<(), Box<dyn Error>>` (or any narrower
   type that widens via `Into`). Use `?` for error propagation.

## What this file deliberately doesn't do

- **No business logic.** Config loading, agent execution, and provider
  I/O all live in submodules.
- **No env-var reads.** The config layer owns that. Once `Config` is
  loaded, the rest of the program never touches `std::env::var`.
- **No fancy error rendering.** `eprintln!("error: {}", e)` is enough
  until users complain.
- **No logging.** No `tracing`, no `log` macros. Adding a logger is a
  separate concern; when it's wanted, it goes here.

## Cross-references

- [config.md](config.md) — how `Config::load()` works.
- [agents.md](agents.md) — what `CoderAgent::run` does today.
- [providers.md](providers.md) — what `NoopProvider` is and what
  replaces it.
- [04-toolchain.md](04-toolchain.md) — why the runtime is `current_thread`.
