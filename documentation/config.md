# `src/config/mod.rs` — config schema, loader, init wizard

421 lines. Owns everything about Parser's user-facing configuration:
the on-disk TOML format, validation rules, the `parser init` wizard,
and two unit tests proving load-time invariants.

The module is the contract between the user (who edits a TOML file)
and the rest of the codebase (which only sees a fully-resolved
`Config` struct).

## On-disk file

Lives at `~/.parser/parser.config.toml` on every platform.
`~` resolves through `dirs::home_dir()`, so the literal path is
`C:\Users\<you>\.parser\parser.config.toml` on Windows,
`/home/<you>/.parser/parser.config.toml` on Linux, etc.

A minimal valid file:

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "moonshotai/kimi-k2"
api_key_env = "OPENROUTER_API_KEY"
```

A full file with every option set:

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "moonshotai/kimi-k2"
api_key_env = "OPENROUTER_API_KEY"

[parameters]
max_tokens     = 4096        # default if omitted
temperature    = 0.7         # default if omitted
context_limit  = 128000      # optional; no default

[paths]
data_dir           = "~/.parser"     # tilde expansion is supported
workspace_data_dir = ".parser"       # relative-to-CWD by convention

[agents]
planner_model     = "anthropic/claude-opus-4"
coder_model       = "moonshotai/kimi-k2"
critic_model      = "openai/gpt-5"
debugger_model    = "openai/gpt-5"
compressor_model  = "moonshotai/kimi-k2"
```

The `[agents]` section is forward-looking: today only `coder_model`
gets used (and even that gets ignored by the placeholder). Each
omitted agent model falls back to `model.name`.

## Two-layer deserialization

The module uses two parallel sets of structs.

### Raw structs — exact on-disk shape

[src/config/mod.rs:58](src/config/mod.rs:58):

```rust
#[derive(Debug, Deserialize)]
struct RawConfig {
    model: Option<RawModel>,
    #[serde(default)]
    parameters: RawParameters,
    #[serde(default)]
    paths: RawPaths,
    #[serde(default)]
    agents: RawAgents,
}
```

Every field on every `Raw*` struct is `Option<T>`, plus `#[serde(default)]`
on whole sections so missing sections deserialize as empty rather than
erroring. This means a TOML file containing only `[model]` parses
cleanly — the rest is filled in by defaults.

### Resolved structs — what the rest of the program sees

[src/config/mod.rs:18](src/config/mod.rs:18):

```rust
pub struct Config {
    pub model: ModelConfig,
    pub parameters: ParametersConfig,
    pub paths: PathsConfig,
    pub agents: AgentsConfig,
}
```

By the time this is constructed, every required field is present,
every default is applied, every `~` is expanded into an absolute
`PathBuf`, and the API key has been read from its env var. **Code
that holds a `Config` does not need to validate it again.**

### Why the split

Three reasons:

1. **Optional-during-deserialization, required-during-use.** Without
   the split, you'd either pollute consuming code with `Option`
   unwraps everywhere, or accept incomplete configs at runtime.
2. **Defaulting in one place.** All defaulting logic lives in
   `from_raw` ([src/config/mod.rs:201](src/config/mod.rs:201)),
   not scattered across consumers.
3. **Path resolution at the boundary.** Strings turn into `PathBuf`s
   at load time — never later — so the rest of the codebase never
   handles `"~/.parser"` as text.

## Public API

[src/config/mod.rs:184](src/config/mod.rs:184):

| Function | Purpose |
|---|---|
| `Config::load()` | Reads from the standard path (`~/.parser/parser.config.toml`). The function `main.rs` calls. |
| `Config::load_from(path: &Path)` | Loads from a caller-supplied path. Used by tests so they don't touch the real config. |
| `home_dir() -> Result<PathBuf, ConfigError>` | Cross-platform home dir, error if unknown. |
| `config_dir()` | `home_dir()/.parser`. |
| `config_file_path()` | `home_dir()/.parser/parser.config.toml`. |
| `init() -> Result<(), ConfigError>` | The interactive wizard. |

`load_from` is split out for testability — the test suite writes
TOML to a `tempfile::tempdir()` and points `load_from` at it, so
unit tests never read or mutate the real `~/.parser/parser.config.toml`.

## The two load-time invariants

These are the contract the loader provides to the rest of the codebase:

### 1. `model.api_key` holds the resolved key, not the env-var name

The TOML file stores `api_key_env = "OPENROUTER_API_KEY"`, not the
key itself — keys never live on disk. At load time the loader reads
`std::env::var(&api_key_env)` and stores the resolved string in
`model.api_key`. After load, the rest of the codebase never reads
env vars for the key.

[src/config/mod.rs:210](src/config/mod.rs:210):

```rust
let api_key = env::var(&api_key_env).map_err(|_| ConfigError::EnvVarNotSet {
    var: api_key_env.clone(),
})?;
```

The test `api_key_field_holds_resolved_value_not_env_var_name` proves
this empirically — see [testing.md](testing.md).

### 2. `paths.data_dir` is an absolute, tilde-expanded `PathBuf`

The TOML may say `data_dir = "~/.parser"`, but `Config::load()`
never returns a `Config` whose `data_dir` is a string with `~` in
it. The conversion happens through `expand_tilde` at
[src/config/mod.rs:169](src/config/mod.rs:169):

```rust
fn expand_tilde(input: &str) -> Result<PathBuf, ConfigError> {
    if let Some(rest) = input.strip_prefix('~') {
        let rest = rest.trim_start_matches(['/', '\\']);
        let mut path = home_dir()?;
        if !rest.is_empty() {
            path.push(rest);
        }
        Ok(path)
    } else {
        Ok(PathBuf::from(input))
    }
}
```

The test `data_dir_is_fully_resolved_pathbuf_not_tilde_string` proves
this empirically.

## Validation rules

From `from_raw` ([src/config/mod.rs:201](src/config/mod.rs:201)) and
its helpers:

| Field | Rule | Failure |
|---|---|---|
| `[model]` section | must exist | `MissingField("model")` |
| `model.endpoint` | must be present, non-empty after trim | `MissingField("model.endpoint")` |
| `model.endpoint` | must be a valid `http://` or `https://` URL with a host | `InvalidUrl { value, reason }` |
| `model.name` | must be present, non-empty after trim | `MissingField("model.name")` |
| `model.api_key_env` | must be present, non-empty after trim | `MissingField("model.api_key_env")` |
| The env var named in `api_key_env` | must be set in the process environment | `EnvVarNotSet { var }` |
| `parameters.max_tokens` | optional; defaults to `4096` | — |
| `parameters.temperature` | optional; defaults to `0.7` | — |
| `parameters.context_limit` | optional; no default (stays `None`) | — |
| `paths.data_dir` | optional; defaults to `~/.parser` | — |
| `paths.workspace_data_dir` | optional; defaults to `.parser` (relative) | — |
| `agents.<role>_model` | each optional; falls back to `model.name` | — |

`require()` at [src/config/mod.rs:257](src/config/mod.rs:257) is the
helper that combines "is present" and "is non-empty after trim" into
one returnable `MissingField` error.

`validate_endpoint()` at [src/config/mod.rs:264](src/config/mod.rs:264)
runs three checks: parses with `url::Url::parse`, asserts the scheme
is `http` or `https`, asserts the URL has a host. Each failure
returns `InvalidUrl { value, reason }` with a different `reason`.

## Error type

[src/config/mod.rs:100](src/config/mod.rs:100):

```rust
pub enum ConfigError {
    NotFound(PathBuf),
    Read(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    MissingField(&'static str),
    InvalidUrl { value: String, reason: String },
    EnvVarNotSet { var: String },
    HomeDirUnknown,
    Io(io::Error),
    Write(PathBuf, io::Error),
}
```

The `Display` impl is hand-written per variant. Each message tells
the user exactly what to do next:

| Variant | What the message says |
|---|---|
| `NotFound` | `"no config found at <path>\n  run `parser init` to create one"` |
| `Read` | `"could not read <path>: <io error>"` |
| `Parse` | `"could not parse <path>: <toml error>"` |
| `MissingField("model.endpoint")` | `"required field `model.endpoint` is missing from parser.config.toml\n  add it under the matching section"` |
| `InvalidUrl` | `"endpoint `<v>` is not a valid URL: <reason>\n  a valid endpoint looks like: https://openrouter.ai/api/v1"` |
| `EnvVarNotSet { var }` | `"environment variable `<var>` is not set\n  set it with: export <var>=\"your-api-key-here\""` |
| `HomeDirUnknown` | `"could not determine your home directory\n  set the HOME environment variable and try again"` |
| `Io` / `Write` | wrap the underlying `io::Error`. |

`impl std::error::Error for ConfigError` is empty (relies on the
default `source` of `None`). That's fine for now; if errors gain
nested causes, the impl can return them via `source()`.

## The `parser init` wizard

[src/config/mod.rs:287](src/config/mod.rs:287). Three questions, in
order, all required:

1. Endpoint URL (validated immediately via `validate_endpoint`).
2. Model name.
3. API-key env-var name.

If a config already exists, the wizard prompts to overwrite (default
is "no" — anything not matching `y`/`yes` aborts cleanly).

After collecting answers, it `create_dir_all`s the parent directory,
writes a minimal config (only `[model]` is included; everything else
falls back to defaults), and prints an `export` hint for the env var.

The two prompt helpers:

- `prompt_raw(message)` — single read from stdin, no validation.
- `prompt(message)` — wraps `prompt_raw` in a loop that re-asks
  until the user types something non-empty after trim.

`render_minimal_config()` formats the TOML with `toml_escape()` to
escape backslashes and double quotes — enough for path values and
API key env var names that might contain odd characters.

The wizard is **not** unit-tested today (stdin-driven prompts are
awkward to fake). It's smoke-tested manually. See
[testing.md](testing.md#gaps).

## Tests

Two unit tests at [src/config/mod.rs:360](src/config/mod.rs:360),
both run via `cargo test --bin parser config::tests`. They prove
the two load-time invariants above. See [testing.md](testing.md)
for full coverage.

## Constants

Top of file ([src/config/mod.rs:9](src/config/mod.rs:9)):

```rust
const CONFIG_DIR_NAME: &str = ".parser";
const CONFIG_FILE_NAME: &str = "parser.config.toml";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const DEFAULT_TEMPERATURE: f32 = 0.7;
const DEFAULT_WORKSPACE_DATA_DIR: &str = ".parser";
```

These are the only magic strings/numbers in the module. Changing the
config file location is a one-line edit here.

## How to extend the schema

Adding a new optional field to an existing section:

1. Add a `pub field_name: T` to the resolved struct (e.g., `ParametersConfig`).
2. Add a `field_name: Option<T>` to the matching `Raw*` struct.
3. In `from_raw`, build the resolved field from the raw `Option`,
   either with `unwrap_or(...)` or `unwrap_or_else(...)`.
4. (Optional) Add a `DEFAULT_FIELD_NAME` constant.
5. (Optional) Add a unit test that asserts the default fires when
   the field is absent.

Adding a new section follows the same pattern with one extra step:
add `#[serde(default)]` to the `RawConfig` field so omitting the
whole section doesn't error.

## What this file deliberately doesn't do

- **No global mutable state.** `Config` is a value, passed around
  explicitly. No `lazy_static`, no `OnceCell`.
- **No reload-on-change.** `Config::load()` is called once, at
  startup. If the file changes mid-run, the running process won't
  see it. That's intentional for now.
- **No env-var fallback for missing config fields.** The only env
  var Parser reads is the one named in `api_key_env`.
- **No JSON or YAML support.** TOML only.
- **No schema versioning.** When the schema changes incompatibly,
  the migration plan goes here.

## Cross-references

- [main.md](main.md) — where `Config::load()` gets called.
- [agents.md](agents.md) — the `[agents]` section's role.
- [providers.md](providers.md) — the future consumer of `model.endpoint`,
  `model.name`, `model.api_key`, and `parameters.*`.
- [testing.md](testing.md) — the two invariant tests, line-by-line.
