use std::io::Write;

use clap::{Parser, Subcommand};
use futures_util::StreamExt;

mod agents;
mod config;
mod providers;

use providers::{Message, ModelProvider, OpenAIProvider, Role};

const SYSTEM_PROMPT: &str = "You are an expert coding assistant. Be concise and direct.";
const MAX_TASK_LEN: usize = 32_768;

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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.command {
        Commands::Init => config::init().map_err(Into::into),
        Commands::Run { task } => run_task(&task.join(" ")).await,
        Commands::External(words) => run_task(&words.join(" ")).await,
    };
    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

/// Execute a coding task end-to-end with live streaming output.
///
/// Loads the validated [`config::Config`], constructs an
/// [`OpenAIProvider`] from it, opens an SSE stream against
/// `{endpoint}/chat/completions`, and prints each incoming chunk
/// to stdout as it arrives — the user sees tokens appear in real
/// time rather than after a long pause.
///
/// The agent layer is bypassed here: streaming straight from the
/// provider keeps the print-on-arrival behaviour the agent's
/// collect-to-string contract can't express. [`agents::CoderAgent`]
/// remains the call path for any caller that wants the full
/// response as a single `String` (tests, programmatic use).
///
/// Output format:
///
/// ```text
/// User: <task>
/// ─────────────────────────────
/// Response: <tokens stream here>
/// ─────────────────────────────
/// ```
///
/// Errors:
/// - Empty / whitespace-only task → `task cannot be empty`, exit 1.
/// - Task longer than [`MAX_TASK_LEN`] → length-and-max message,
///   exit 1.
/// - Auth / API / network / stream errors from the provider are
///   caught and printed via the top-level handler in [`main`].
/// - Mid-stream `Err(ProviderError)` ends the stream, prints
///   `Stream interrupted. Partial response shown above.` if any
///   bytes had already been streamed, exit 1.
/// - Empty completion (stream ended cleanly with no chunks) prints
///   `The model returned an empty response. Try rephrasing your
///   task.`, exit 1.
async fn run_task(task: &str) -> Result<(), Box<dyn std::error::Error>> {
    let trimmed = task.trim();
    if trimmed.is_empty() {
        eprintln!("error: task cannot be empty");
        std::process::exit(1);
    }
    if trimmed.len() > MAX_TASK_LEN {
        eprintln!(
            "error: task is {} characters, maximum is {}",
            trimmed.len(),
            MAX_TASK_LEN
        );
        std::process::exit(1);
    }

    let cfg = config::Config::load()?;
    let provider = OpenAIProvider::from_config(&cfg);

    let messages = vec![
        Message {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
        },
        Message {
            role: Role::User,
            content: trimmed.to_string(),
        },
    ];

    let divider = "─".repeat(29);
    println!("User: {}", trimmed);
    println!("{}", divider);

    // Open the stream first so a pre-stream failure (auth /
    // network / API) propagates via `?` cleanly, without leaving
    // a half-printed "Response: " line on stdout.
    let mut stream = provider.stream_completion(messages).await?;
    print!("Response: ");
    std::io::stdout().flush().ok();

    let mut collected = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                print!("{}", chunk);
                std::io::stdout().flush().ok();
                collected.push_str(&chunk);
            }
            Err(e) => {
                println!();
                eprintln!("error: {}", e);
                if !collected.is_empty() {
                    eprintln!("Stream interrupted. Partial response shown above.");
                }
                std::process::exit(1);
            }
        }
    }

    println!();
    if collected.is_empty() {
        eprintln!("The model returned an empty response. Try rephrasing your task.");
        std::process::exit(1);
    }
    println!("{}", divider);

    Ok(())
}
