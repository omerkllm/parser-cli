# CLI reference

The binary is named `parser`. CLI argument parsing is done by `clap` with
the derive API in [src/main.rs](src/main.rs).

## Subcommands

### `parser init`

Interactive wizard that creates `~/.parser/parser.config.toml`.

It asks three questions:

1. What is your provider endpoint URL? (e.g. `https://openrouter.ai/api/v1`)
2. What model do you want to use? (e.g. `moonshotai/kimi-k2`)
3. What environment variable holds your API key? (e.g. `OPENROUTER_API_KEY`)

It validates the endpoint as a real http/https URL **before** writing the
file (so a typo doesn't get persisted), creates `~/.parser/` if it doesn't
exist, writes a minimal config containing only those three answers, and
prints the `export` command you need to run to set the key.

If the config file already exists, `init` asks before overwriting.

### `parser run "<task>"`

Runs a coding task. The task can be a single quoted string or multiple
words — `clap` collects them into a `Vec<String>` and the handler joins
with spaces, so these two are equivalent:

```
parser run "fix the jwt bug"
parser run fix the jwt bug
```

`run` requires at least one word. `parser run` with no task is rejected by
clap with usage info.

What `run` does **today**:

1. Calls `Config::load()`.
2. If loading fails, prints the error and exits with status 1.
3. If loading succeeds, prints a four-line confirmation:

   ```
   Config loaded successfully
   Model: <cfg.model.name>
   Endpoint: <cfg.model.endpoint>
   Ready. Provider and agent coming in next step.
   ```

The task itself is received and discarded for now. Step 2 (provider) and
step 3 (agent) will pick it up.

### Free-form (no subcommand)

Anything that isn't a recognized subcommand becomes a task too:

```
parser "fix the jwt bug"
parser hello world
```

Both of these route to the same handler as `parser run ...`.

This is implemented via clap's `external_subcommand` mechanism: the `Cli`
struct sets `allow_external_subcommands = true`, and the `Commands` enum
has a catch-all variant `External(Vec<String>)` annotated with
`#[command(external_subcommand)]`. Any unrecognized first word is captured
along with its arguments into that variant, and `main()` joins the words
with spaces and dispatches the same way.

### `parser` with no arguments

Prints help and exits with status 2 (clap's convention for "no command
given, help shown"). The `Cli` struct uses
`#[command(arg_required_else_help = true)]` to make this happen.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Application error (bad config, missing env var, IO error, invalid URL, etc.) — error message printed to stderr |
| `2` | Argument error from clap, or help shown for missing args |

`std::process::exit(1)` is called explicitly from `main()` only when the
handler returns an `Err(ConfigError)`.

## Error messages

All application errors come from the `ConfigError` enum and have hand-tuned
`Display` implementations. Examples:

```
$ parser run "fix bug"          # no config file
error: no config found at C:\Users\omerk\.parser\parser.config.toml
  run `parser init` to create one
```

```
$ parser run "fix bug"          # OPENROUTER_API_KEY not exported
error: environment variable `OPENROUTER_API_KEY` is not set
  set it with: export OPENROUTER_API_KEY="your-api-key-here"
```

```
$ parser init                   # user typed an invalid URL
error: endpoint `not a url` is not a valid URL: relative URL without a base
  a valid endpoint looks like: https://openrouter.ai/api/v1
```

The exact strings are defined in `impl fmt::Display for ConfigError` in
[src/config/mod.rs](src/config/mod.rs:107).

## Adding a new subcommand later

To add e.g. `parser status`:

1. Add a variant to the `Commands` enum in [src/main.rs](src/main.rs).
2. Add a match arm in `main()` that calls a handler function.
3. Implement the handler. If it can fail with a `ConfigError`, return
   `Result<(), ConfigError>` and the existing error printing in `main()`
   takes care of it.

Don't add a separate `mod` for trivial commands — keep them in `main.rs`
or in `src/config/mod.rs`. Only split out a module when a command grows
real logic (e.g. `src/agent/mod.rs` later for `parser run`).
