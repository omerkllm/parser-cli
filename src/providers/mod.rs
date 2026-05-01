#![allow(dead_code)]
#![allow(clippy::enum_variant_names)]

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

/// The role of a message in a chat-completions conversation.
///
/// Maps directly to the OpenAI chat-completions wire format. The
/// three named variants — `System`, `User`, `Assistant` — cover
/// the standard schema and serialize to the lowercase strings
/// `"system"`, `"user"`, `"assistant"`. `Other(String)` is the
/// escape hatch for vendor-specific roles a provider may need to
/// round-trip without the type having to anticipate them
/// (`"tool"`, `"function"`, `"developer"`, etc.).
///
/// When building a request body where the role might be `Other`,
/// use `role.to_string()` (the [`Display`](std::fmt::Display) impl)
/// rather than `serde_json::to_value(&role)`. The derived
/// `Serialize` produces `{"Other":"..."}` for the tuple variant —
/// fine for an internal decision-log dump, wrong for the wire.
#[derive(Debug, Clone, Serialize)]
pub enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    Other(String),
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => f.write_str("system"),
            Role::User => f.write_str("user"),
            Role::Assistant => f.write_str("assistant"),
            Role::Other(s) => f.write_str(s.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => Role::Other(s),
        })
    }
}

/// A single turn in a chat-completions conversation.
///
/// `role` and `content` map directly to the OpenAI chat-completions
/// wire format: each entry in the request's `messages` array is a
/// `{"role": "...", "content": "..."}` object. Used both as input
/// to [`ModelProvider::stream_completion`] and as the element type
/// of [`crate::agents::AgentInput::conversation_history`].
#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug)]
pub enum ProviderError {
    /// The endpoint returned a non-2xx HTTP response. The inner
    /// string carries the response body (or a useful slice of it)
    /// — concrete enough to copy-paste into a bug report.
    ApiError(String),
    /// The request never reached the endpoint, or the connection
    /// dropped mid-flight: DNS failure, TLS handshake error,
    /// refused connection, timeout.
    NetworkError(String),
    /// 401 / 403 from the endpoint, or the API-key env var was
    /// missing at config-load time. Surfaced separately from
    /// `ApiError` so the CLI can suggest `parser init` or
    /// `export <YOUR_API_KEY>=...` instead of dumping a raw body.
    AuthError(String),
    /// The request succeeded and a stream began, but a chunk
    /// failed to parse — malformed SSE, dropped mid-response,
    /// encoding mismatch.
    StreamError(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::ApiError(msg) => write!(f, "api error: {}", msg),
            ProviderError::NetworkError(msg) => write!(f, "network error: {}", msg),
            ProviderError::AuthError(msg) => write!(f, "auth error: {}", msg),
            ProviderError::StreamError(msg) => write!(f, "stream error: {}", msg),
        }
    }
}

impl std::error::Error for ProviderError {}

/// The interface every AI provider must implement.
///
/// Parser is provider-agnostic: any OpenAI-compatible endpoint
/// works (OpenRouter, Ollama, Groq, Together AI, LM Studio, etc.).
/// This trait is the single seam through which agents talk to
/// models — a real provider only has to implement
/// [`stream_completion`](ModelProvider::stream_completion).
///
/// [`NoopProvider`] is the temporary compile stub used until the
/// real OpenAI-compatible provider lands in the next step. Once
/// that lands, `NoopProvider` is deleted and replaced.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Send a conversation to the model and stream the response
    /// back chunk-by-chunk.
    ///
    /// Streaming is the chosen interface for three reasons:
    ///
    /// 1. **Latency to first token.** The user sees output almost
    ///    immediately, rather than after a long pause while the
    ///    full response is generated.
    /// 2. **Cancellation.** Dropping the returned stream closes
    ///    the underlying HTTP connection — no wasted compute or
    ///    API credits when a task is aborted.
    /// 3. **Composability.** Downstream layers (compressor,
    ///    decision log) can react to chunks as they arrive instead
    ///    of waiting for end-of-response.
    ///
    /// A simpler `complete()` method that returns the full string
    /// at once will be added later as a convenience for callers
    /// (tests, batch jobs) that don't care about streaming.
    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError>;
}

/// Temporary compile stub. Exists only to satisfy the type system
/// while no real provider is implemented — `main` needs *some*
/// `ModelProvider` value to pass to `agent.run()`. Returns an
/// empty stream from every call. Deleted in the next step when
/// the real OpenAI-compatible provider lands.
pub struct NoopProvider;

#[async_trait]
impl ModelProvider for NoopProvider {
    async fn stream_completion(
        &self,
        _messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}
