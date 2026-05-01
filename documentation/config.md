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

From `from_raw` and its validation helpers
(`validate_endpoint`, `validate_model_name`,
`validate_api_key_env_name`, `validate_api_key_value`,
`validate_parameters`):

| Field | Rule | Failure |
|---|---|---|
| `[model]` section | must exist | `MissingField("model")` |
| `model.endpoint` | must be present, non-empty after trim | `MissingField("model.endpoint")` |
| `model.endpoint` | must be a valid `http://` or `https://` URL with a host | `InvalidUrl { value, reason }` |
| `model.endpoint` | trailing `/` is **stripped silently** after URL validation, so consumers can append paths like `/chat/completions` without producing double slashes on the wire. `https://x.com/api/v1/` becomes `https://x.com/api/v1`. | — (normalization, not failure) |
| `model.endpoint` | a trailing `/chat/completions` is also **stripped silently** after the slash strip, so a user who pastes the full chat-completions endpoint URL gets the base URL the provider layer expects. `https://api.openai.com/v1/chat/completions` (with or without a further trailing `/`) becomes `https://api.openai.com/v1`. The provider appends the path itself when building requests. | — (normalization, not failure) |
| `model.name` | must be present, non-empty after trim | `MissingField("model.name")` |
| `model.name` | must not be only whitespace (defensive check inside `validate_model_name` — the `require()` helper already catches whitespace-only inputs in the normal flow with `MissingField`, but `validate_model_name` enforces the same invariant in isolation) | `InvalidField { field: "model.name", reason: "must not be empty or contain only whitespace" }` |
| `model.name` | length must not exceed 200 characters | `InvalidField { field: "model.name", reason }` |
| `model.api_key_env` | must be present, non-empty after trim | `MissingField("model.api_key_env")` |
| `model.api_key_env` | name must contain no whitespace (env-var names with spaces can't be set via standard shell syntax) | `InvalidField { field: "model.api_key_env", reason: "must not contain whitespace" }` |
| `model.api_key_env` | length must not exceed 200 characters | `InvalidField { field: "model.api_key_env", reason }` |
| The env var named in `api_key_env` | must be set in the process environment | `EnvVarNotSet { var }` |
| **Resolved API key value** | must not be empty after trimming whitespace | `InvalidApiKey { reason: "value is empty after trimming whitespace" }` |
| **Resolved API key value** | must contain no `\n` or `\r` (catches the most common copy-paste mistake — accidentally including the trailing newline from terminal output) | `InvalidApiKey { reason: "value contains a newline or carriage return — common copy-paste mistake; re-export the variable on a single line" }` |
| **Resolved API key value** | must not start or end with a `"` character. Catches the common Windows / PowerShell mistake of running `$env:KEY = '"sk-or-v1-..."'` or `set KEY="sk-or-v1-..."` — the literal quotes get stored as part of the value, and the provider then sends `Authorization: Bearer "sk-..."` (with quotes), which every endpoint rejects. The fix is to set the variable without quotes: `export KEY=sk-or-v1-...` (or `$env:KEY = "sk-or-v1-..."` in PowerShell, where the outer `"..."` is the syntax, not the value). | `InvalidApiKey { reason: "value contains surrounding quotes — set the key without quotes: export KEY=value" }` |
| `parameters.max_tokens` | optional; defaults to `4096`. When set, must be in `1..=32768`. | `InvalidField { field: "parameters.max_tokens", reason }` |
| `parameters.temperature` | optional; defaults to `0.7`. When set, must be in `0.0..=2.0`. | `InvalidField { field: "parameters.temperature", reason }` |
| `parameters.context_limit` | optional; no default (stays `None`). When `Some`, must be in `1..=2_000_000`. | `InvalidField { field: "parameters.context_limit", reason }` |
| `parameters.context_limit` | when `Some`, must additionally be **strictly greater than `max_tokens`**. A context window narrower than (or equal to) the output cap is logically invalid — there'd be no room left for the input prompt. The error message echoes the current `max_tokens` so the user knows what to compare against. | `InvalidField { field: "parameters.context_limit", reason: "must be greater than max_tokens (<value>)" }` |
| `paths.data_dir` | optional; defaults to `~/.parser` | — |
| `paths.workspace_data_dir` | optional; defaults to `.parser` (relative) | — |
| `agents.<role>_model` | each optional; falls back to `model.name` | — |

`require()` is the helper that combines "is present" and "is
non-empty after trim" into one returnable `MissingField` error.

`validate_endpoint()` runs three checks: parses with
`url::Url::parse`, asserts the scheme is `http` or `https`,
asserts the URL has a host. Each failure returns `InvalidUrl
{ value, reason }` with a different `reason`. Trailing-slash
stripping happens *after* `validate_endpoint` returns `Ok`, in
`from_raw`.

`validate_model_name`, `validate_api_key_env_name`,
`validate_api_key_value`, and `validate_parameters` each
return `InvalidField { field, reason }` (or
`InvalidApiKey { reason }`) when their checks fail. The `field`
strings — `"model.name"`, `"parameters.temperature"`, etc. —
are stable identifiers that callers can match on.

The numeric ranges live as named constants near the top of the
file: `MAX_MODEL_NAME_LEN`, `MAX_API_KEY_ENV_LEN`,
`MIN_TEMPERATURE` / `MAX_TEMPERATURE`,
`MIN_MAX_TOKENS` / `MAX_MAX_TOKENS`,
`MIN_CONTEXT_LIMIT` / `MAX_CONTEXT_LIMIT`. Adjust them in one
place if a future model demands a wider range.

## Error type

[src/config/mod.rs:100](src/config/mod.rs:100):

```rust
pub enum ConfigError {
    NotFound(PathBuf),
    Read(PathBuf, io::Error),
    Parse(PathBuf, toml::de::Error),
    MissingField(&'static str),
    InvalidField { field: &'static str, reason: String },
    InvalidUrl { value: String, reason: String },
    InvalidApiKey { reason: String },
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
| `InvalidField { field, reason }` | `"invalid value for `<field>`: <reason>"` — used for length-bounded fields (`model.name`, `model.api_key_env`) and numeric range checks (`parameters.temperature`, `parameters.max_tokens`, `parameters.context_limit`). |
| `InvalidUrl` | `"endpoint `<v>` is not a valid URL: <reason>\n  a valid endpoint looks like: https://openrouter.ai/api/v1"` |
| `InvalidApiKey { reason }` | `"invalid API key: <reason>"` — fired when the resolved env-var value is blank after trimming, contains `\n` / `\r`, or starts/ends with a literal `"` (the Windows-shell quoting mistake). The `reason` is specific enough to copy-paste into a bug report. |
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

### Atomic write

The config write goes through `write_config_atomically(path, body)`,
not a direct `fs::write`. The helper:

1. Writes the body to `<path>.tmp` (e.g.
   `~/.parser/parser.config.toml.tmp`). If this fails partway —
   disk full, permission error, process killed mid-write — the
   partial `.tmp` is removed before the error is returned, so a
   half-written file is never left behind.
2. `fs::rename`s `<path>.tmp` onto `<path>`. Rename is atomic on
   every supported filesystem when both paths sit on the same
   volume (which they do here — both live under `~/.parser/`).
   The user observes either the old `parser.config.toml` or the
   fully-written new one, never an intermediate state.
3. If the rename itself fails, the `.tmp` is also cleaned up
   before returning, so a retried `parser init` doesn't trip over
   a stale temp file.

This matters for the overwrite path in particular: a user with an
existing working config who runs `parser init` and accepts the
overwrite prompt is guaranteed not to lose their old config to a
partially-written replacement, even if the process is killed
between the prompt and the final rename.

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

Fourteen unit tests in `#[cfg(test)] mod tests`, run via
`cargo test --bin parser config::tests` (`cargo test --bin parser`
runs them along with the agent tests for a total of 16):

| Test | Proves |
|---|---|
| `api_key_field_holds_resolved_value_not_env_var_name` | The loader resolves the env-var value into `model.api_key`, not the env-var *name*. |
| `data_dir_is_fully_resolved_pathbuf_not_tilde_string` | A `~/...` path is expanded to an absolute `PathBuf` with no literal `~`. |
| `endpoint_trailing_slash_is_stripped_silently` | `https://x/api/v1/` is normalized to `https://x/api/v1` after URL validation. |
| `model_name_longer_than_max_returns_invalid_field` | A model name beyond `MAX_MODEL_NAME_LEN` is rejected with `InvalidField`. |
| `whitespace_only_model_name_is_rejected_by_validate` | `validate_model_name("   \t  ")` returns `InvalidField` for `model.name` — proves the helper's whitespace-only guard in isolation, even though the `from_raw` flow's `require()` short-circuits the same input as `MissingField`. |
| `api_key_env_containing_whitespace_returns_invalid_field` | An env-var name with spaces is rejected before any env lookup. |
| `api_key_env_longer_than_max_returns_invalid_field` | An env-var name beyond `MAX_API_KEY_ENV_LEN` is rejected. |
| `blank_api_key_returns_invalid_api_key` | A resolved key that's whitespace-only is rejected with `InvalidApiKey`. |
| `api_key_with_newline_returns_invalid_api_key` | A resolved key containing `\n` is rejected with `InvalidApiKey`. |
| `api_key_with_surrounding_quotes_returns_invalid_api_key` | A resolved key starting and ending with `"` (e.g. `"sk-or-v1-abc"`) is rejected with `InvalidApiKey` — catches the Windows-shell quoting mistake. |
| `temperature_out_of_range_returns_invalid_field` | `temperature = 3.0` is rejected with `InvalidField` targeting `parameters.temperature`. |
| `max_tokens_out_of_range_returns_invalid_field` | `max_tokens = 0` is rejected with `InvalidField` targeting `parameters.max_tokens`. |
| `context_limit_out_of_range_returns_invalid_field` | `context_limit = 3_000_000` is rejected with `InvalidField` targeting `parameters.context_limit`. |
| `context_limit_below_max_tokens_returns_invalid_field` | `context_limit = 1000` with `max_tokens = 4096` is rejected with `InvalidField` targeting `parameters.context_limit` — proves the strict `context_limit > max_tokens` requirement. |

Each validation test that needs a resolved API key uses a unique
`PARSER_TEST_KEY_INVARIANT_<N>` env var (numbered 1 through 11)
so parallel test execution doesn't race on `std::env::set_var`.
The whitespace-only model-name test calls `validate_model_name`
directly and needs no env var. See [testing.md](testing.md) for
the test infrastructure pattern.

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
