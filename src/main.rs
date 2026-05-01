use clap::{Parser, Subcommand};

mod agents;
mod config;
mod providers;

use agents::{Agent, AgentInput, CoderAgent};
use providers::NoopProvider;

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

/// Execute a coding task end-to-end.
///
/// Today this loads the user's config, prints a confirmation of
/// the model + endpoint that will be used, instantiates a
/// [`CoderAgent`] paired with a [`NoopProvider`], and runs the
/// agent — which currently just returns the placeholder string
/// `"Coder agent placeholder"`. So today's output is four lines
/// of confirmation regardless of the task input.
///
/// The next step replaces [`NoopProvider`] with the real
/// OpenAI-compatible provider and replaces `CoderAgent::run`'s
/// placeholder body with a real call to
/// [`ModelProvider::stream_completion`](crate::providers::ModelProvider::stream_completion).
/// The body of this function does not change — the provider is
/// already trait-dispatched, so swapping the concrete type is
/// the only edit needed at the call site.
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
