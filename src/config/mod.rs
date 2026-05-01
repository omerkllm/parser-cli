#![allow(dead_code)]
#![allow(clippy::result_large_err)]

use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

const CONFIG_DIR_NAME: &str = ".parser";
const CONFIG_FILE_NAME: &str = "parser.config.toml";

const DEFAULT_MAX_TOKENS: u32 = 4096;
const DEFAULT_TEMPERATURE: f32 = 0.7;
const DEFAULT_WORKSPACE_DATA_DIR: &str = ".parser";

const MAX_MODEL_NAME_LEN: usize = 200;
const MAX_API_KEY_ENV_LEN: usize = 200;
const MIN_TEMPERATURE: f32 = 0.0;
const MAX_TEMPERATURE: f32 = 2.0;
const MIN_MAX_TOKENS: u32 = 1;
const MAX_MAX_TOKENS: u32 = 32_768;
const MIN_CONTEXT_LIMIT: u32 = 1;
const MAX_CONTEXT_LIMIT: u32 = 2_000_000;

// ---------- resolved config ----------

#[derive(Debug, Clone)]
pub struct Config {
    pub model: ModelConfig,
    pub parameters: ParametersConfig,
    pub paths: PathsConfig,
    pub agents: AgentsConfig,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub endpoint: String,
    pub name: String,
    pub api_key_env: String,
    pub api_key: String,
}

#[derive(Debug, Clone)]
pub struct ParametersConfig {
    pub max_tokens: u32,
    pub temperature: f32,
    pub context_limit: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PathsConfig {
    pub data_dir: PathBuf,
    pub workspace_data_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub planner_model: String,
    pub coder_model: String,
    pub critic_model: String,
    pub debugger_model: String,
    pub compressor_model: String,
}

// ---------- raw on-disk shape ----------

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

#[derive(Debug, Deserialize)]
struct RawModel {
    endpoint: Option<String>,
    name: Option<String>,
    api_key_env: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawParameters {
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    context_limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPaths {
    data_dir: Option<String>,
    workspace_data_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAgents {
    planner_model: Option<String>,
    coder_model: Option<String>,
    critic_model: Option<String>,
    debugger_model: Option<String>,
    compressor_model: Option<String>,
}

// ---------- errors ----------

#[derive(Debug)]
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

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NotFound(path) => write!(
                f,
                "no config found at {}\n  run `parser init` to create one",
                path.display()
            ),
            ConfigError::Read(path, e) => {
                write!(f, "could not read {}: {}", path.display(), e)
            }
            ConfigError::Parse(path, e) => {
                write!(f, "could not parse {}: {}", path.display(), e)
            }
            ConfigError::MissingField(field) => write!(
                f,
                "required field `{}` is missing from parser.config.toml\n  add it under the matching section",
                field
            ),
            ConfigError::InvalidField { field, reason } => write!(
                f,
                "invalid value for `{}`: {}",
                field, reason
            ),
            ConfigError::InvalidUrl { value, reason } => write!(
                f,
                "endpoint `{}` is not a valid URL: {}\n  a valid endpoint looks like: https://openrouter.ai/api/v1",
                value, reason
            ),
            ConfigError::InvalidApiKey { reason } => write!(
                f,
                "invalid API key: {}",
                reason
            ),
            ConfigError::EnvVarNotSet { var } => write!(
                f,
                "environment variable `{var}` is not set\n  set it with: export {var}=\"your-api-key-here\""
            ),
            ConfigError::HomeDirUnknown => write!(
                f,
                "could not determine your home directory\n  set the HOME environment variable and try again"
            ),
            ConfigError::Io(e) => write!(f, "io error: {}", e),
            ConfigError::Write(path, e) => {
                write!(f, "could not write {}: {}", path.display(), e)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

// ---------- path helpers ----------

pub fn home_dir() -> Result<PathBuf, ConfigError> {
    dirs::home_dir().ok_or(ConfigError::HomeDirUnknown)
}

pub fn config_dir() -> Result<PathBuf, ConfigError> {
    Ok(home_dir()?.join(CONFIG_DIR_NAME))
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join(CONFIG_FILE_NAME))
}

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

// ---------- loader ----------

impl Config {
    /// Load and validate `~/.parser/parser.config.toml`.
    ///
    /// Two-layer loading strategy:
    ///
    /// 1. **Raw deserialize** — TOML is read off disk into
    ///    [`RawConfig`], a permissive shape where every field is
    ///    `Option<T>`. This layer's only job is to mirror the
    ///    on-disk structure; it does no validation.
    /// 2. **Resolve** — [`Config::from_raw`] converts the raw
    ///    shape into the strict [`Config`] used by the rest of
    ///    the program: required fields are checked (returning
    ///    [`ConfigError::MissingField`]), defaults are applied
    ///    (`max_tokens = 4096`, `temperature = 0.7`,
    ///    per-agent model = the global model name), `~`-prefixed
    ///    paths are expanded to absolute [`PathBuf`]s, the
    ///    endpoint URL is validated, and the API key is
    ///    **resolved from its env var** so consumers read
    ///    `cfg.model.api_key` directly instead of re-checking
    ///    `std::env::var` every call site.
    ///
    /// After this returns `Ok`, every field on `Config` is
    /// guaranteed present, validated, and ready to use — there is
    /// no second validation step elsewhere in the codebase.
    pub fn load() -> Result<Self, ConfigError> {
        let path = config_file_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_path_buf()));
        }
        let text =
            fs::read_to_string(path).map_err(|e| ConfigError::Read(path.to_path_buf(), e))?;
        let raw: RawConfig =
            toml::from_str(&text).map_err(|e| ConfigError::Parse(path.to_path_buf(), e))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, ConfigError> {
        let model_raw = raw.model.ok_or(ConfigError::MissingField("model"))?;

        let endpoint = require(model_raw.endpoint, "model.endpoint")?;
        let name = require(model_raw.name, "model.name")?;
        let api_key_env = require(model_raw.api_key_env, "model.api_key_env")?;

        validate_endpoint(&endpoint)?;
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let endpoint = match endpoint.strip_suffix("/chat/completions") {
            Some(stripped) => stripped.to_string(),
            None => endpoint,
        };

        validate_model_name(&name)?;
        validate_api_key_env_name(&api_key_env)?;

        let api_key = env::var(&api_key_env).map_err(|_| ConfigError::EnvVarNotSet {
            var: api_key_env.clone(),
        })?;
        validate_api_key_value(&api_key)?;

        let model = ModelConfig {
            endpoint,
            name: name.clone(),
            api_key_env,
            api_key,
        };

        let parameters = ParametersConfig {
            max_tokens: raw.parameters.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            temperature: raw.parameters.temperature.unwrap_or(DEFAULT_TEMPERATURE),
            context_limit: raw.parameters.context_limit,
        };
        validate_parameters(&parameters)?;

        let data_dir = match raw.paths.data_dir {
            Some(s) => expand_tilde(&s)?,
            None => config_dir()?,
        };
        let workspace_data_dir = match raw.paths.workspace_data_dir {
            Some(s) => expand_tilde(&s)?,
            None => PathBuf::from(DEFAULT_WORKSPACE_DATA_DIR),
        };
        let paths = PathsConfig {
            data_dir,
            workspace_data_dir,
        };

        let agents = AgentsConfig {
            planner_model: raw.agents.planner_model.unwrap_or_else(|| name.clone()),
            coder_model: raw.agents.coder_model.unwrap_or_else(|| name.clone()),
            critic_model: raw.agents.critic_model.unwrap_or_else(|| name.clone()),
            debugger_model: raw.agents.debugger_model.unwrap_or_else(|| name.clone()),
            compressor_model: raw.agents.compressor_model.unwrap_or_else(|| name.clone()),
        };

        Ok(Config {
            model,
            parameters,
            paths,
            agents,
        })
    }
}

fn require(value: Option<String>, field: &'static str) -> Result<String, ConfigError> {
    match value {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(ConfigError::MissingField(field)),
    }
}

fn validate_endpoint(endpoint: &str) -> Result<(), ConfigError> {
    let url = url::Url::parse(endpoint).map_err(|e| ConfigError::InvalidUrl {
        value: endpoint.to_string(),
        reason: e.to_string(),
    })?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::InvalidUrl {
            value: endpoint.to_string(),
            reason: format!("scheme `{}` is not http or https", scheme),
        });
    }
    if !url.has_host() {
        return Err(ConfigError::InvalidUrl {
            value: endpoint.to_string(),
            reason: "missing host".to_string(),
        });
    }
    Ok(())
}

fn validate_model_name(name: &str) -> Result<(), ConfigError> {
    if name.trim().is_empty() {
        return Err(ConfigError::InvalidField {
            field: "model.name",
            reason: "must not be empty or contain only whitespace".to_string(),
        });
    }
    if name.len() > MAX_MODEL_NAME_LEN {
        return Err(ConfigError::InvalidField {
            field: "model.name",
            reason: format!(
                "name is {} characters, maximum is {}",
                name.len(),
                MAX_MODEL_NAME_LEN
            ),
        });
    }
    Ok(())
}

fn validate_api_key_env_name(name: &str) -> Result<(), ConfigError> {
    if name.chars().any(char::is_whitespace) {
        return Err(ConfigError::InvalidField {
            field: "model.api_key_env",
            reason: "must not contain whitespace".to_string(),
        });
    }
    if name.len() > MAX_API_KEY_ENV_LEN {
        return Err(ConfigError::InvalidField {
            field: "model.api_key_env",
            reason: format!(
                "name is {} characters, maximum is {}",
                name.len(),
                MAX_API_KEY_ENV_LEN
            ),
        });
    }
    Ok(())
}

fn validate_api_key_value(api_key: &str) -> Result<(), ConfigError> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::InvalidApiKey {
            reason: "value is empty after trimming whitespace".to_string(),
        });
    }
    if api_key.contains('\n') || api_key.contains('\r') {
        return Err(ConfigError::InvalidApiKey {
            reason: "value contains a newline or carriage return — common copy-paste mistake; re-export the variable on a single line".to_string(),
        });
    }
    if trimmed.starts_with('"') || trimmed.ends_with('"') {
        return Err(ConfigError::InvalidApiKey {
            reason:
                "value contains surrounding quotes — set the key without quotes: export KEY=value"
                    .to_string(),
        });
    }
    Ok(())
}

fn validate_parameters(p: &ParametersConfig) -> Result<(), ConfigError> {
    if !(MIN_TEMPERATURE..=MAX_TEMPERATURE).contains(&p.temperature) {
        return Err(ConfigError::InvalidField {
            field: "parameters.temperature",
            reason: format!(
                "{} is outside the allowed range {}..={}",
                p.temperature, MIN_TEMPERATURE, MAX_TEMPERATURE
            ),
        });
    }
    if !(MIN_MAX_TOKENS..=MAX_MAX_TOKENS).contains(&p.max_tokens) {
        return Err(ConfigError::InvalidField {
            field: "parameters.max_tokens",
            reason: format!(
                "{} is outside the allowed range {}..={}",
                p.max_tokens, MIN_MAX_TOKENS, MAX_MAX_TOKENS
            ),
        });
    }
    if let Some(cl) = p.context_limit {
        if !(MIN_CONTEXT_LIMIT..=MAX_CONTEXT_LIMIT).contains(&cl) {
            return Err(ConfigError::InvalidField {
                field: "parameters.context_limit",
                reason: format!(
                    "{} is outside the allowed range {}..={}",
                    cl, MIN_CONTEXT_LIMIT, MAX_CONTEXT_LIMIT
                ),
            });
        }
        if cl <= p.max_tokens {
            return Err(ConfigError::InvalidField {
                field: "parameters.context_limit",
                reason: format!("must be greater than max_tokens ({})", p.max_tokens),
            });
        }
    }
    Ok(())
}

// ---------- init command ----------

pub fn init() -> Result<(), ConfigError> {
    let dir = config_dir()?;
    let path = config_file_path()?;

    if path.exists() {
        eprintln!("config already exists at {}", path.display());
        let answer = prompt_raw("overwrite? [y/N] ")?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("aborted");
            return Ok(());
        }
    }

    let endpoint =
        prompt("What is your provider endpoint URL?\n  example: https://openrouter.ai/api/v1\n> ")?;
    validate_endpoint(&endpoint)?;

    let model_name = prompt("What model do you want to use?\n  example: moonshotai/kimi-k2\n> ")?;

    let api_key_env =
        prompt("What environment variable holds your API key?\n  example: OPENROUTER_API_KEY\n> ")?;

    fs::create_dir_all(&dir).map_err(|e| ConfigError::Write(dir.clone(), e))?;
    let body = render_minimal_config(&endpoint, &model_name, &api_key_env);
    write_config_atomically(&path, &body)?;

    println!();
    println!("wrote config to {}", path.display());
    println!();
    println!("next: set the API key environment variable so parser can read it");
    println!("  export {}=\"your-api-key-here\"", api_key_env);
    Ok(())
}

/// Write the config body atomically: write to `<path>.tmp` first,
/// then rename onto `path`. If the write fails partway through,
/// the partial `.tmp` file is removed before returning, so a
/// killed-mid-write process can never leave behind a corrupt
/// `parser.config.toml`. The rename is atomic on every supported
/// filesystem; the user will see either the old config or the
/// new one, never a half-written intermediate.
fn write_config_atomically(path: &Path, body: &str) -> Result<(), ConfigError> {
    let mut tmp_name = path
        .file_name()
        .expect("config file path has a file name")
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = path.with_file_name(tmp_name);

    if let Err(e) = fs::write(&tmp_path, body) {
        let _ = fs::remove_file(&tmp_path);
        return Err(ConfigError::Write(tmp_path, e));
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(ConfigError::Write(path.to_path_buf(), e));
    }

    Ok(())
}

fn prompt(message: &str) -> Result<String, ConfigError> {
    loop {
        let value = prompt_raw(message)?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
        println!("(value cannot be empty)");
    }
}

fn prompt_raw(message: &str) -> Result<String, ConfigError> {
    print!("{}", message);
    io::stdout().flush().map_err(ConfigError::Io)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(ConfigError::Io)?;
    Ok(input)
}

fn render_minimal_config(endpoint: &str, name: &str, api_key_env: &str) -> String {
    format!(
        "[model]\n\
         endpoint = \"{}\"\n\
         name = \"{}\"\n\
         api_key_env = \"{}\"\n",
        toml_escape(endpoint),
        toml_escape(name),
        toml_escape(api_key_env),
    )
}

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("parser.config.toml");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    /// Proves that `Config::load_from` resolves the API-key env
    /// var into `model.api_key` rather than just copying the
    /// env-var *name* into it. The third assertion (`assert_ne!`)
    /// is the load-bearing one: if the loader ever regresses to
    /// copying `api_key_env` into `api_key`, this test fails
    /// immediately. Matters because every consumer reads
    /// `model.api_key` directly and never re-checks env vars.
    ///
    /// Uses `load_from(&Path)` instead of `load()` so the test
    /// reads from a temp dir created by `tempfile::tempdir()` —
    /// `load()` would touch the user's real
    /// `~/.parser/parser.config.toml`, which tests must never do.
    #[test]
    fn api_key_field_holds_resolved_value_not_env_var_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "moonshotai/kimi-k2"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_1"
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_1", "sk-or-v1-FAKE-abc123");

        let cfg = Config::load_from(&path).expect("load");

        assert_eq!(cfg.model.api_key_env, "PARSER_TEST_KEY_INVARIANT_1");
        assert_eq!(cfg.model.api_key, "sk-or-v1-FAKE-abc123");
        assert_ne!(cfg.model.api_key, cfg.model.api_key_env);
    }

    /// Proves that a trailing `/` on the configured endpoint is
    /// stripped silently after URL validation, so consumers of
    /// `cfg.model.endpoint` can append paths like `/chat/completions`
    /// without producing double slashes on the wire.
    #[test]
    fn endpoint_trailing_slash_is_stripped_silently() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1/"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_3"
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_3", "k");

        let cfg = Config::load_from(&path).expect("load");

        assert_eq!(cfg.model.endpoint, "https://openrouter.ai/api/v1");
    }

    /// Proves that a `model.name` longer than `MAX_MODEL_NAME_LEN`
    /// characters is rejected with `ConfigError::InvalidField`,
    /// targeting the right field. Guards against pathological
    /// input — a 10 MB string accidentally pasted in — reaching
    /// the provider request body.
    #[test]
    fn model_name_longer_than_max_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let long_name = "x".repeat(MAX_MODEL_NAME_LEN + 1);
        let body = format!(
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "{}"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_4"
            "#,
            long_name
        );
        let path = write_config(tmp.path(), &body);
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_4", "k");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "model.name"),
            "expected InvalidField for model.name, got: {:?}",
            err
        );
    }

    /// Proves that an `api_key_env` value containing whitespace is
    /// rejected. Such names cannot be set via standard shell syntax
    /// (`export FOO BAR=...` is a syntax error) — failing fast at
    /// load time saves a confusing `EnvVarNotSet` error later.
    #[test]
    fn api_key_env_containing_whitespace_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER TEST KEY"
            "#,
        );

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "model.api_key_env"),
            "expected InvalidField for model.api_key_env, got: {:?}",
            err
        );
    }

    /// Proves that an `api_key_env` longer than
    /// `MAX_API_KEY_ENV_LEN` is rejected with `InvalidField`.
    #[test]
    fn api_key_env_longer_than_max_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let long_name = "X".repeat(MAX_API_KEY_ENV_LEN + 1);
        let body = format!(
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "{}"
            "#,
            long_name
        );
        let path = write_config(tmp.path(), &body);

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "model.api_key_env"),
            "expected InvalidField for model.api_key_env, got: {:?}",
            err
        );
    }

    /// Proves that a resolved API key consisting of only whitespace
    /// is rejected with `InvalidApiKey`. Catches the common case of
    /// the env var being set but empty — without this check the
    /// provider would receive a blank `Authorization` header and
    /// return a confusing 401.
    #[test]
    fn blank_api_key_returns_invalid_api_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_5"
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_5", "   \t  ");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidApiKey { .. }),
            "expected InvalidApiKey, got: {:?}",
            err
        );
    }

    /// Proves that an API key containing a newline is rejected
    /// with `InvalidApiKey`. Catches the most common copy-paste
    /// mistake — accidentally including the trailing `\n` from
    /// terminal output.
    #[test]
    fn api_key_with_newline_returns_invalid_api_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_6"
            "#,
        );
        std::env::set_var(
            "PARSER_TEST_KEY_INVARIANT_6",
            "sk-or-v1-real-part\n-trailing",
        );

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidApiKey { .. }),
            "expected InvalidApiKey, got: {:?}",
            err
        );
    }

    /// Proves that a `temperature` outside `MIN_TEMPERATURE..=MAX_TEMPERATURE`
    /// is rejected with `InvalidField` targeting `parameters.temperature`.
    #[test]
    fn temperature_out_of_range_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_7"

            [parameters]
            temperature = 3.0
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_7", "k");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "parameters.temperature"),
            "expected InvalidField for parameters.temperature, got: {:?}",
            err
        );
    }

    /// Proves that a `max_tokens` outside `MIN_MAX_TOKENS..=MAX_MAX_TOKENS`
    /// is rejected with `InvalidField` targeting `parameters.max_tokens`.
    #[test]
    fn max_tokens_out_of_range_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_8"

            [parameters]
            max_tokens = 0
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_8", "k");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "parameters.max_tokens"),
            "expected InvalidField for parameters.max_tokens, got: {:?}",
            err
        );
    }

    /// Proves that a `context_limit` outside
    /// `MIN_CONTEXT_LIMIT..=MAX_CONTEXT_LIMIT` is rejected with
    /// `InvalidField` targeting `parameters.context_limit`. The
    /// validation runs only when `context_limit` is `Some`, so a
    /// missing field still picks up the (no) default.
    #[test]
    fn context_limit_out_of_range_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_9"

            [parameters]
            context_limit = 3000000
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_9", "k");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "parameters.context_limit"),
            "expected InvalidField for parameters.context_limit, got: {:?}",
            err
        );
    }

    /// Proves that an API key value with surrounding double-quote
    /// characters is rejected with `InvalidApiKey`. Catches the
    /// common Windows mistake of running `set KEY="value"` (or
    /// `KEY="value"` in PowerShell) which stores the literal
    /// quotes as part of the value.
    #[test]
    fn api_key_with_surrounding_quotes_returns_invalid_api_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_10"
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_10", "\"sk-or-v1-abc\"");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidApiKey { .. }),
            "expected InvalidApiKey, got: {:?}",
            err
        );
    }

    /// Proves that `validate_model_name` rejects whitespace-only
    /// names with `InvalidField` targeting `model.name`. Trimming
    /// happens inside the helper, so `"   \t  "` is treated the
    /// same as `""`.
    #[test]
    fn whitespace_only_model_name_is_rejected_by_validate() {
        let result = validate_model_name("   \t  ");

        assert!(
            matches!(result, Err(ConfigError::InvalidField { field, .. }) if field == "model.name"),
            "expected InvalidField for model.name, got: {:?}",
            result
        );
    }

    /// Proves that a `context_limit` smaller than or equal to
    /// `max_tokens` is rejected with `InvalidField`. A context
    /// window narrower than the output cap is logically invalid:
    /// no room for the input would be left.
    #[test]
    fn context_limit_below_max_tokens_returns_invalid_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_11"

            [parameters]
            max_tokens = 4096
            context_limit = 1000
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_11", "k");

        let err = Config::load_from(&path).unwrap_err();

        assert!(
            matches!(err, ConfigError::InvalidField { field, .. } if field == "parameters.context_limit"),
            "expected InvalidField for parameters.context_limit, got: {:?}",
            err
        );
    }

    /// Proves that a `~/...` path in the config is expanded to an
    /// absolute [`PathBuf`] before `Config` is returned, with no
    /// literal `~` left in the resolved value. Matters because
    /// every later filesystem-touching component (indexer cache,
    /// decision log, etc.) takes a `&Path` argument and assumes
    /// it's already resolved — and `~` is not a shell expansion
    /// on Windows, so an unresolved tilde would silently fail.
    ///
    /// Like the other test, uses `load_from(&Path)` against a
    /// `tempfile::tempdir()` to avoid touching the user's real
    /// config at `~/.parser/parser.config.toml`.
    #[test]
    fn data_dir_is_fully_resolved_pathbuf_not_tilde_string() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_config(
            tmp.path(),
            r#"
            [model]
            endpoint = "https://openrouter.ai/api/v1"
            name = "x"
            api_key_env = "PARSER_TEST_KEY_INVARIANT_2"

            [paths]
            data_dir = "~/.parser"
            "#,
        );
        std::env::set_var("PARSER_TEST_KEY_INVARIANT_2", "k");

        let cfg = Config::load_from(&path).expect("load");

        let home = dirs::home_dir().expect("home");
        assert_eq!(cfg.paths.data_dir, home.join(".parser"));
        assert!(cfg.paths.data_dir.is_absolute(), "must be absolute");
        assert!(
            !cfg.paths.data_dir.to_string_lossy().contains('~'),
            "tilde must be expanded, got {:?}",
            cfg.paths.data_dir
        );
    }
}
