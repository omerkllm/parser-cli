// The Coder agent and its support types are exercised by unit
// tests in this module and are part of the library surface, but
// `main.rs` streams directly through the provider for live token
// output. From the binary's perspective these items are dead, so
// silence the warnings module-wide rather than per-item.
#![allow(dead_code)]

use futures_util::StreamExt;

use crate::providers::{Message, ModelProvider, ProviderError, Role};

const MAX_TASK_LEN: usize = 32_768;

const SYSTEM_PROMPT: &str = "You are an expert coding assistant. Be concise and direct.";

pub struct AgentInput {
    pub task: String,
    pub conversation_history: Vec<Message>,
}

#[derive(Debug)]
pub struct AgentOutput {
    pub response: String,
}

/// Error type returned by [`Agent::run`].
///
/// Each variant covers a distinct failure mode an agent run can
/// hit. `AgentError` widens cleanly into `Box<dyn std::error::Error>`
/// in [`crate::main`](crate)'s top-level handler, so callers don't
/// need bespoke conversion code.
#[derive(Debug)]
pub enum AgentError {
    /// The provider returned an error during completion. Wraps the
    /// underlying [`ProviderError`] so the original network / API /
    /// auth / stream context isn't lost. `From<ProviderError>` is
    /// implemented for this variant, so `?` on a provider call
    /// inside `run` propagates the failure with no manual `map_err`.
    ProviderFailed(ProviderError),
    /// The model returned a response that the agent could not use:
    /// malformed JSON in a structured-output mode, an empty body
    /// where content was required, a schema mismatch on a tool call,
    /// etc. The inner string describes what was wrong.
    InvalidResponse(String),
    /// The conversation history plus the new task exceeded the
    /// context-window budget for the configured model. The agent
    /// declined to send a request that was guaranteed to be
    /// truncated. Carries no payload — the recovery is the same in
    /// every case: drop history or shorten the task.
    ContextLimitExceeded,
    /// The task string was empty after trimming. Surfaced before any
    /// provider call so an empty prompt never leaves the binary.
    TaskEmpty,
    /// The task string (after trimming) exceeded the agent's
    /// length budget. Carries the actual length and the configured
    /// maximum so the user knows by how much they need to shorten.
    TaskTooLong { length: usize, max: usize },
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::ProviderFailed(e) => write!(f, "provider error: {}", e),
            AgentError::InvalidResponse(s) => {
                write!(f, "agent received invalid response: {}", s)
            }
            AgentError::ContextLimitExceeded => f.write_str("context limit exceeded"),
            AgentError::TaskEmpty => f.write_str("task cannot be empty"),
            AgentError::TaskTooLong { length, max } => {
                write!(f, "task is {} characters, maximum is {}", length, max)
            }
        }
    }
}

impl std::error::Error for AgentError {}

impl From<ProviderError> for AgentError {
    fn from(e: ProviderError) -> Self {
        AgentError::ProviderFailed(e)
    }
}

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

/// The Coder agent. Writes code and answers programming questions.
///
/// Today this is the only agent with a real implementation. It:
///
/// 1. Validates the task (trim, non-empty, length under the cap).
/// 2. Builds the message list — system prompt first, then any
///    `conversation_history` from the input, then the new user
///    turn. The provider sends this list verbatim; nothing else
///    is prepended on the wire.
/// 3. Opens a streaming completion against the provider and
///    accumulates the chunks into `AgentOutput.response`. A
///    mid-stream `Err(ProviderError)` short-circuits with
///    `AgentError::ProviderFailed`.
/// 4. Surfaces `AgentError::InvalidResponse` if the model returned
///    no content at all.
pub struct CoderAgent;

impl CoderAgent {
    pub fn new() -> Self {
        CoderAgent
    }
}

impl Default for CoderAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for CoderAgent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError> {
        let trimmed = input.task.trim();
        if trimmed.is_empty() {
            return Err(AgentError::TaskEmpty);
        }
        if trimmed.len() > MAX_TASK_LEN {
            return Err(AgentError::TaskTooLong {
                length: trimmed.len(),
                max: MAX_TASK_LEN,
            });
        }

        let mut messages: Vec<Message> = Vec::with_capacity(input.conversation_history.len() + 2);
        messages.push(Message {
            role: Role::System,
            content: SYSTEM_PROMPT.to_string(),
        });
        messages.extend(input.conversation_history);
        messages.push(Message {
            role: Role::User,
            content: trimmed.to_string(),
        });

        let mut stream = provider.stream_completion(messages).await?;
        let mut collected = String::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => collected.push_str(&chunk),
                Err(e) => return Err(AgentError::ProviderFailed(e)),
            }
        }

        if collected.is_empty() {
            return Err(AgentError::InvalidResponse(
                "model returned empty response".to_string(),
            ));
        }

        Ok(AgentOutput {
            response: collected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::Stream;
    use std::pin::Pin;

    /// In-test provider that never gets called by the existing
    /// validation tests — they all bail out before reaching the
    /// provider. Exists only to satisfy the `&dyn ModelProvider`
    /// argument now that `NoopProvider` is gone.
    struct StubProvider;

    #[async_trait]
    impl ModelProvider for StubProvider {
        async fn complete(&self, _messages: Vec<Message>) -> Result<String, ProviderError> {
            Ok(String::new())
        }

        async fn stream_completion(
            &self,
            _messages: Vec<Message>,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>, ProviderError>
        {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    /// Proves that a task containing only whitespace is treated
    /// as empty after `.trim()` and surfaces as
    /// `AgentError::TaskEmpty`. Without trimming first, a string
    /// like "    " would slip past the empty check and reach the
    /// provider — which is exactly what the validation prevents.
    #[tokio::test]
    async fn whitespace_only_task_returns_task_empty() {
        let agent = CoderAgent::new();
        let provider = StubProvider;
        let input = AgentInput {
            task: "   \t \n  ".to_string(),
            conversation_history: Vec::new(),
        };

        let err = agent.run(input, &provider).await.unwrap_err();

        assert!(
            matches!(err, AgentError::TaskEmpty),
            "expected TaskEmpty, got: {:?}",
            err
        );
    }

    /// Proves that a task longer than `MAX_TASK_LEN` characters
    /// (after trimming) is rejected with
    /// `AgentError::TaskTooLong`, carrying both the actual length
    /// and the configured maximum so the error message shows the
    /// user exactly how much they need to trim.
    #[tokio::test]
    async fn task_longer_than_max_returns_task_too_long() {
        let agent = CoderAgent::new();
        let provider = StubProvider;
        let big_task = "x".repeat(MAX_TASK_LEN + 1);
        let input = AgentInput {
            task: big_task,
            conversation_history: Vec::new(),
        };

        let err = agent.run(input, &provider).await.unwrap_err();

        match err {
            AgentError::TaskTooLong { length, max } => {
                assert_eq!(length, MAX_TASK_LEN + 1);
                assert_eq!(max, MAX_TASK_LEN);
            }
            other => panic!("expected TaskTooLong, got: {:?}", other),
        }
    }
}
