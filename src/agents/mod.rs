#![allow(dead_code)]

use crate::providers::{Message, ModelProvider};

pub struct AgentInput {
    pub task: String,
    pub conversation_history: Vec<Message>,
}

pub struct AgentOutput {
    pub response: String,
}

/// Error type returned by [`Agent::run`].
///
/// Empty by design: a placeholder agent cannot fail, so there are
/// no variants yet. Real failure modes — provider errors, parse
/// failures, tool-call errors — get added as variants when the
/// real `CoderAgent::run` body lands in the next step. Keeping the
/// type in place now means the trait signature is stable; adding
/// variants later is a non-breaking change for callers that
/// already match exhaustively against `Result<_, AgentError>`.
#[derive(Debug)]
pub enum AgentError {}

impl std::fmt::Display for AgentError {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

impl std::error::Error for AgentError {}

/// The shared interface for all five planned agents.
///
/// Parser's roadmap has five agent roles, each potentially backed
/// by a different model (per-agent overrides live in
/// `parser.config.toml`'s `[agents]` section):
///
/// - `Planner` — decomposes a task into steps.
/// - `Coder` — writes the code.
/// - `Critic` — reviews the coder's output.
/// - `Debugger` — investigates failures.
/// - `Compressor` — summarizes context for later turns.
///
/// All five share this trait shape, so adding a new agent later
/// is a single `impl Agent for ...` block.
///
/// `run` is a native `async fn` rather than going through
/// `#[async_trait]` because agents are statically dispatched (we
/// always know the concrete agent type at the call site, e.g.
/// `CoderAgent::run`). No `&dyn Agent` is needed today, so the
/// dyn-compatibility cost of `#[async_trait]` (heap-allocated
/// boxed futures) is avoidable. Native async fn in traits has
/// been stable since Rust 1.75.
pub trait Agent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError>;
}

/// Placeholder Coder agent. Returns the literal string
/// `"Coder agent placeholder"` from `run` and ignores both
/// arguments — the underscore prefixes on `_input` and `_provider`
/// are intentional, suppressing "unused argument" warnings until
/// the real body wires them up.
///
/// The real implementation lands in the next step: build a system
/// prompt, append `input.conversation_history` and the new task
/// turn, call `provider.stream_completion(...)`, collect the
/// chunks into `AgentOutput.response`. The trait signature stays
/// the same — only this `impl` block changes.
pub struct CoderAgent;

impl CoderAgent {
    pub fn new() -> Self {
        CoderAgent
    }
}

impl Agent for CoderAgent {
    async fn run(
        &self,
        _input: AgentInput,
        _provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError> {
        Ok(AgentOutput {
            response: "Coder agent placeholder".to_string(),
        })
    }
}
