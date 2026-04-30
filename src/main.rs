use clap::{Parser, Subcommand};

mod config;

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

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Init => config::init(),
        Commands::Run { task } => run_task(&task.join(" ")),
        Commands::External(words) => run_task(&words.join(" ")),
    };
    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run_task(_task: &str) -> Result<(), config::ConfigError> {
    let cfg = config::Config::load()?;
    println!("Config loaded successfully");
    println!("Model: {}", cfg.model.name);
    println!("Endpoint: {}", cfg.model.endpoint);
    println!("Ready. Provider and agent coming in next step.");
    Ok(())
}
