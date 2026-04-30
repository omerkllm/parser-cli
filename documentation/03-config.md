# Config system

All config logic lives in [src/config/mod.rs](src/config/mod.rs). The user-
facing config file is `~/.parser/parser.config.toml` in TOML format.

## File format

Four sections. `[model]` is required; the others are optional with defaults.

### `[model]` — required

| Field | Type | Example |
|---|---|---|
| `endpoint` | string (http/https URL) | `https://openrouter.ai/api/v1` |
| `name` | string | `moonshotai/kimi-k2` |
| `api_key_env` | string (env-var name) | `OPENROUTER_API_KEY` |

The API key itself is **not** stored in the config file. The config holds
the *name* of an environment variable; the loader reads the value at
runtime. This keeps secrets out of disk-resident files and out of any
backup that captures them.

### `[parameters]` — optional

| Field | Default | Notes |
|---|---|---|
| `max_tokens` | `4096` | per-request output cap |
| `temperature` | `0.7` | float |
| `context_limit` | `None` | `None` means "use the model's reported maximum"; set an integer to clamp lower |

### `[paths]` — optional

| Field | Default | Notes |
|---|---|---|
| `data_dir` | `~/.parser` | global state (indices, embeddings, decision logs) |
| `workspace_data_dir` | `.parser` | per-project state (relative to repo root) |

`~` is expanded to the user's home directory at load time. Both fields
become absolute `PathBuf` once loaded — see "Resolution" below.

### `[agents]` — optional

| Field | Default |
|---|---|
| `planner_model` | `[model].name` |
| `coder_model` | `[model].name` |
| `critic_model` | `[model].name` |
| `debugger_model` | `[model].name` |
| `compressor_model` | `[model].name` |

Any role you don't specify falls back to the top-level model. This means
you can run a single-model setup by leaving `[agents]` empty, or split per
role later without touching `[model]`.

## Minimal config

The `parser init` wizard writes only the three required fields. The rest
take their defaults at load time:

```toml
[model]
endpoint = "https://openrouter.ai/api/v1"
name = "moonshotai/kimi-k2"
api_key_env = "OPENROUTER_API_KEY"
```

## The `Config` struct

After `Config::load()` succeeds, you get a single `Config` value with
everything **fully resolved**:

```rust
pub struct Config {
    pub model: ModelConfig,
    pub parameters: ParametersConfig,
    pub paths: PathsConfig,
    pub agents: AgentsConfig,
}

pub struct ModelConfig {
    pub endpoint: String,        // validated http/https URL
    pub name: String,
    pub api_key_env: String,     // the *name* of the env var
    pub api_key: String,         // the *value* read from that env var
}

pub struct ParametersConfig {
    pub max_tokens: u32,
    pub temperature: f32,
    pub context_limit: Option<u32>,
}

pub struct PathsConfig {
    pub data_dir: PathBuf,           // absolute, ~ expanded
    pub workspace_data_dir: PathBuf, // absolute or repo-relative
}

pub struct AgentsConfig {
    pub planner_model: String,
    pub coder_model: String,
    pub critic_model: String,
    pub debugger_model: String,
    pub compressor_model: String,
}
```

## Loader semantics — the two key invariants

### Invariant 1: `model.api_key` is the resolved value, not the env-var name

```rust
let api_key = env::var(&api_key_env)
    .map_err(|_| ConfigError::EnvVarNotSet { var: api_key_env.clone() })?;

let model = ModelConfig {
    api_key_env,    // "OPENROUTER_API_KEY"
    api_key,        // "sk-or-v1-abc123..."
    ...
};
```

Step 2's HTTP client will read `cfg.model.api_key` directly. There is no
second env-var lookup.

If the env var is unset, `Config::load()` returns
`ConfigError::EnvVarNotSet` whose `Display` includes the export command
the user needs to run.

### Invariant 2: `paths.data_dir` is a fully expanded `PathBuf`

```rust
let data_dir = match raw.paths.data_dir {
    Some(s) => expand_tilde(&s)?,    // "~/.parser" → "C:/Users/omerk/.parser"
    None    => config_dir()?,        // dirs::home_dir().join(".parser")
};
```

Both branches go through `dirs::home_dir()`, so the resulting `PathBuf` is
always absolute. No code path lets a literal `~` survive into the
resolved struct. If the home directory cannot be determined, `load()`
returns `ConfigError::HomeDirUnknown` rather than papering over it with a
relative path.

Both invariants are verified empirically by the unit tests — see
[05-testing.md](documentation/05-testing.md).

## Validation rules

The loader enforces these and produces specific error variants for each:

| Rule | Error variant | What user sees |
|---|---|---|
| Config file missing | `NotFound(PathBuf)` | path + "run `parser init`" |
| TOML parse fails | `Parse(PathBuf, toml::de::Error)` | path + toml error |
| Required field missing | `MissingField(&'static str)` | exact field name (`model.endpoint` etc.) |
| Endpoint not http/https URL | `InvalidUrl { value, reason }` | shows valid example |
| API-key env var unset | `EnvVarNotSet { var }` | shows `export VAR=...` |
| Home dir undeterminable | `HomeDirUnknown` | suggests setting `HOME` |
| File read/write IO error | `Read` / `Write` / `Io` | path + os error |

## Public API

Anything you might call from elsewhere in the codebase:

```rust
// Loading
config::Config::load() -> Result<Config, ConfigError>
config::Config::load_from(&Path) -> Result<Config, ConfigError>

// Path helpers
config::home_dir() -> Result<PathBuf, ConfigError>
config::config_dir() -> Result<PathBuf, ConfigError>
config::config_file_path() -> Result<PathBuf, ConfigError>

// Init wizard
config::init() -> Result<(), ConfigError>
```

`load_from` is what the test suite uses to point the loader at a
temp-directory config without touching the user's real one.

## Where each piece lives in the source

| Concern | Location |
|---|---|
| Resolved struct types | [src/config/mod.rs:18](src/config/mod.rs:18) |
| Raw TOML deserialization types | [src/config/mod.rs:64](src/config/mod.rs:64) |
| `ConfigError` enum + `Display` | [src/config/mod.rs:106](src/config/mod.rs:106) |
| `home_dir` / `config_dir` / `config_file_path` | [src/config/mod.rs:155](src/config/mod.rs:155) |
| `expand_tilde` | [src/config/mod.rs:169](src/config/mod.rs:169) |
| `Config::load_from` and `from_raw` | [src/config/mod.rs:184](src/config/mod.rs:184) |
| `validate_endpoint` | [src/config/mod.rs:255](src/config/mod.rs:255) |
| `parser init` wizard | [src/config/mod.rs:276](src/config/mod.rs:276) |
| Unit tests (the two invariants) | [src/config/mod.rs:361](src/config/mod.rs:361) |

(Line numbers may drift as code grows; these are accurate as of step 1.)

## Editing the config by hand

Once `init` has written the minimal file, you can extend it manually. A
fully-populated example:

```toml
[model]
endpoint = "https://openrouter.ai/api/v1"
name = "moonshotai/kimi-k2"
api_key_env = "OPENROUTER_API_KEY"

[parameters]
max_tokens = 8192
temperature = 0.3
context_limit = 200000

[paths]
data_dir = "~/.parser"
workspace_data_dir = ".parser"

[agents]
planner_model    = "anthropic/claude-sonnet-4-6"
coder_model      = "moonshotai/kimi-k2"
critic_model     = "anthropic/claude-opus-4-7"
debugger_model   = "moonshotai/kimi-k2"
compressor_model = "anthropic/claude-haiku-4-5"
```

Note: when `[agents]` references different models, Step 2's provider
implementation will still use the **same endpoint and API key** for all
of them. Multi-endpoint support (different providers per role) is not
yet planned — flag it if you want it.
