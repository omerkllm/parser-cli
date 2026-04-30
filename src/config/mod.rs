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
    InvalidUrl { value: String, reason: String },
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
            ConfigError::InvalidUrl { value, reason } => write!(
                f,
                "endpoint `{}` is not a valid URL: {}\n  a valid endpoint looks like: https://openrouter.ai/api/v1",
                value, reason
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
    pub fn load() -> Result<Self, ConfigError> {
        let path = config_file_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_path_buf()));
        }
        let text = fs::read_to_string(path)
            .map_err(|e| ConfigError::Read(path.to_path_buf(), e))?;
        let raw: RawConfig = toml::from_str(&text)
            .map_err(|e| ConfigError::Parse(path.to_path_buf(), e))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, ConfigError> {
        let model_raw = raw.model.ok_or(ConfigError::MissingField("model"))?;

        let endpoint = require(model_raw.endpoint, "model.endpoint")?;
        let name = require(model_raw.name, "model.name")?;
        let api_key_env = require(model_raw.api_key_env, "model.api_key_env")?;

        validate_endpoint(&endpoint)?;

        let api_key = env::var(&api_key_env).map_err(|_| ConfigError::EnvVarNotSet {
            var: api_key_env.clone(),
        })?;

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

    let endpoint = prompt(
        "What is your provider endpoint URL?\n  example: https://openrouter.ai/api/v1\n> ",
    )?;
    validate_endpoint(&endpoint)?;

    let model_name = prompt(
        "What model do you want to use?\n  example: moonshotai/kimi-k2\n> ",
    )?;

    let api_key_env = prompt(
        "What environment variable holds your API key?\n  example: OPENROUTER_API_KEY\n> ",
    )?;

    fs::create_dir_all(&dir).map_err(|e| ConfigError::Write(dir.clone(), e))?;
    let body = render_minimal_config(&endpoint, &model_name, &api_key_env);
    fs::write(&path, body).map_err(|e| ConfigError::Write(path.clone(), e))?;

    println!();
    println!("wrote config to {}", path.display());
    println!();
    println!("next: set the API key environment variable so parser can read it");
    println!("  export {}=\"your-api-key-here\"", api_key_env);
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
    use std::io::Write as _;

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("parser.config.toml");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

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
