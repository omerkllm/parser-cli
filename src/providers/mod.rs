#![allow(clippy::enum_variant_names)]

use std::collections::VecDeque;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// [`complete`](ModelProvider::complete).
///
/// **Caller owns the system message.** The provider serializes
/// whatever `Vec<Message>` it receives, including any system /
/// developer turns the caller chose to prepend. Nothing is added
/// implicitly.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Send a conversation to the model and return the full
    /// response as a `String`. A single blocking POST: the request
    /// goes out, the full response comes back, no incremental
    /// output. Convenient for tests, batch jobs, and any caller
    /// that doesn't care about latency-to-first-token.
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError>;

    /// Stream the response back chunk-by-chunk over Server-Sent
    /// Events.
    ///
    /// Streaming is the right interface for the interactive CLI
    /// path because:
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
    /// Each yielded item is a `Result<String, ProviderError>`. An
    /// `Err` mid-stream means the chunk could not be parsed or the
    /// connection dropped — the caller decides whether to abort,
    /// retry, or surface a partial response. The stream ends
    /// naturally on the SSE `[DONE]` sentinel or when the
    /// underlying byte stream is exhausted.
    ///
    /// The default implementation calls
    /// [`complete`](ModelProvider::complete) and yields the full
    /// response as a single-item stream — correct behaviour, but
    /// none of the latency or cancellation benefits above. Real
    /// streaming providers override this.
    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>, ProviderError>
    {
        let response = self.complete(messages).await?;
        Ok(Box::pin(futures::stream::once(async move { Ok(response) })))
    }
}

/// OpenAI-compatible chat-completions provider.
///
/// Works with any endpoint that speaks the OpenAI chat-completions
/// wire format: OpenRouter, OpenAI itself, Groq, Together AI,
/// Ollama, LM Studio, and others. The endpoint URL, model name,
/// API key, and sampling parameters all come from the user's
/// `~/.parser/parser.config.toml` via [`Config`] — construct an
/// `OpenAIProvider` with [`OpenAIProvider::from_config`] once at
/// startup and reuse it for every request.
///
/// `HTTP-Referer` and `X-Title` are sent on every request. They're
/// optional in the OpenAI spec but OpenRouter uses them to
/// identify and rank apps in their public leaderboard — sending
/// them is best practice for any tool talking to OpenRouter.
pub struct OpenAIProvider {
    endpoint: String,
    model: String,
    api_key: String,
    max_tokens: u32,
    temperature: f32,
    client: reqwest::Client,
}

impl OpenAIProvider {
    /// Build a provider from a validated [`Config`]. Clones the
    /// four config fields the provider needs and constructs a fresh
    /// `reqwest::Client`. Cheap; call once per process.
    pub fn from_config(cfg: &Config) -> Self {
        OpenAIProvider {
            endpoint: cfg.model.endpoint.clone(),
            model: cfg.model.name.clone(),
            api_key: cfg.model.api_key.clone(),
            max_tokens: cfg.parameters.max_tokens,
            temperature: cfg.parameters.temperature,
            // 10s connect_timeout: covers DNS + TLS handshake.
            // 120s timeout: covers the full request including
            // streaming — long enough for a slow model, short
            // enough that a hung connection doesn't block forever.
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Common request body for both [`complete`] and
    /// [`stream_completion`]. The `stream` field is the only
    /// difference between the two.
    fn build_body(&self, messages: &[Message], stream: bool) -> Value {
        let messages_json: Vec<Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role.to_string(),
                    "content": m.content,
                })
            })
            .collect();
        json!({
            "model": self.model,
            "messages": messages_json,
            "stream": stream,
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
        })
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.endpoint)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }
}

/// Typed shape of a single OpenAI-compatible SSE `data:` chunk.
/// Only the fields we actually read are declared; everything
/// else (id, created, model, finish_reason, etc.) is ignored.
///
/// `delta.content` is `Option<String>` because providers commonly
/// emit chunks where the field is JSON `null` — typically the
/// first chunk (which only carries the assistant's role marker)
/// and the last chunk before `[DONE]` (which carries
/// `finish_reason` but no new text). With the bare `Value`
/// traversal we used to do, this worked but obscured intent;
/// the typed parse makes "content might be null, skip it" the
/// explicit shape of the data.
#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<SseChoice>,
}

#[derive(Deserialize)]
struct SseChoice {
    #[serde(default)]
    delta: SseDelta,
}

#[derive(Deserialize, Default)]
struct SseDelta {
    #[serde(default)]
    content: Option<String>,
}

/// Map a non-2xx HTTP response to a [`ProviderError`]. Status-code
/// rules in one place so `complete` and `stream_completion` agree.
async fn map_error_response(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    match status.as_u16() {
        401 => {
            ProviderError::AuthError("invalid API key — run parser init to reconfigure".to_string())
        }
        402 => ProviderError::ApiError(
            "insufficient credits — add credits at openrouter.ai/credits".to_string(),
        ),
        429 => ProviderError::ApiError("rate limited — wait a moment and try again".to_string()),
        _ => {
            let body = response
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read body: {}>", e));
            ProviderError::ApiError(format!("HTTP {}: {}", status, body))
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAIProvider {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError> {
        if self.api_key.is_empty() {
            return Err(ProviderError::AuthError(
                "no API key configured — run parser init".to_string(),
            ));
        }
        let body = self.build_body(&messages, false);
        let response = self
            .client
            .post(self.url())
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/omerkllm/parser-cli")
            .header("X-Title", "parser-cli")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::ApiError(format!("malformed JSON response: {}", e)))?;

        json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ProviderError::ApiError("response missing choices[0].message.content".to_string())
            })
    }

    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>, ProviderError>
    {
        if self.api_key.is_empty() {
            return Err(ProviderError::AuthError(
                "no API key configured — run parser init".to_string(),
            ));
        }
        let body = self.build_body(&messages, true);
        let response = self
            .client
            .post(self.url())
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/omerkllm/parser-cli")
            .header("X-Title", "parser-cli")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        // Box::pin the underlying byte stream so it's `Unpin`,
        // which lets us call `.next().await` on it inside the
        // unfold closure without pin-projecting through the tuple
        // state.
        let byte_stream = Box::pin(response.bytes_stream());

        // State threaded through `unfold` as a tuple — type is
        // inferred so we never have to name `bytes::Bytes`:
        //   .0 byte_stream      pinned reqwest byte stream
        //   .1 line_buffer      bytes that arrived without a
        //                       trailing '\n' yet
        //   .2 pending          parsed items not yet yielded (one
        //                       network chunk can contain multiple
        //                       SSE events; we buffer and drain
        //                       one per unfold step)
        //   .3 done             set once we hit `data: [DONE]` so
        //                       the stream ends on the next poll
        let initial = (
            byte_stream,
            String::new(),
            VecDeque::<Result<String, ProviderError>>::new(),
            false,
        );

        let stream = futures_util::stream::unfold(initial, |mut state| async move {
            loop {
                if let Some(item) = state.2.pop_front() {
                    return Some((item, state));
                }
                if state.3 {
                    return None;
                }

                match state.0.next().await {
                    Some(Ok(bytes)) => {
                        let chunk = String::from_utf8_lossy(&bytes);
                        state.1.push_str(&chunk);

                        while let Some(newline_idx) = state.1.find('\n') {
                            let line: String = state.1.drain(..=newline_idx).collect();
                            let line = line.trim_end_matches(['\r', '\n']);
                            if line.is_empty() || line.starts_with(':') {
                                continue;
                            }
                            let payload = match line.strip_prefix("data: ") {
                                Some(p) => p,
                                None => continue,
                            };
                            if payload == "[DONE]" {
                                state.3 = true;
                                continue;
                            }
                            // Typed parse: `delta.content` is
                            // `Option<String>` so a JSON null
                            // (common on first/last chunks from
                            // many providers) deserializes to
                            // `None` instead of panicking. Empty
                            // strings are also filtered before
                            // yielding.
                            match serde_json::from_str::<SseChunk>(payload) {
                                Ok(parsed) => {
                                    if let Some(content) = parsed
                                        .choices
                                        .into_iter()
                                        .next()
                                        .and_then(|c| c.delta.content)
                                    {
                                        if !content.is_empty() {
                                            state.2.push_back(Ok(content));
                                        }
                                    }
                                }
                                Err(e) => {
                                    state.2.push_back(Err(ProviderError::StreamError(format!(
                                        "malformed SSE chunk: {}",
                                        e
                                    ))));
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        state.3 = true;
                        return Some((Err(ProviderError::NetworkError(e.to_string())), state));
                    }
                    None => {
                        state.3 = true;
                        // Trailing content in line_buffer is dropped:
                        // a well-formed SSE stream always ends with
                        // a complete `data: [DONE]\n` line.
                        if state.2.is_empty() {
                            return None;
                        }
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build an `OpenAIProvider` pointed at a wiremock server.
    fn mock_provider(server: &MockServer) -> OpenAIProvider {
        OpenAIProvider {
            endpoint: server.uri(),
            model: "test-model".to_string(),
            api_key: "test-key".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            client: reqwest::Client::new(),
        }
    }

    /// Proves that a 200 response with a well-formed
    /// chat-completions body yields the inner content string from
    /// `choices[0].message.content`.
    #[tokio::test]
    async fn complete_returns_content_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "content": "hello" } }]
            })))
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let result = provider
            .complete(vec![Message {
                role: Role::User,
                content: "hi".to_string(),
            }])
            .await;

        assert_eq!(result.unwrap(), "hello");
    }

    /// Proves that a 401 response is mapped to
    /// `ProviderError::AuthError`, not `ApiError`. The CLI relies
    /// on this distinction to suggest `parser init` instead of
    /// dumping a raw body.
    #[tokio::test]
    async fn complete_returns_auth_error_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let err = provider.complete(vec![]).await.unwrap_err();

        assert!(
            matches!(err, ProviderError::AuthError(_)),
            "expected AuthError, got: {:?}",
            err
        );
    }

    /// Proves that a 429 response is mapped to
    /// `ProviderError::ApiError` with the rate-limit message.
    #[tokio::test]
    async fn complete_returns_api_error_on_429() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let err = provider.complete(vec![]).await.unwrap_err();

        assert!(
            matches!(err, ProviderError::ApiError(ref s) if s.contains("rate limited")),
            "expected ApiError with 'rate limited', got: {:?}",
            err
        );
    }

    /// Proves that the SSE stream parser yields chunks in order
    /// from the `data:` lines and stops cleanly at `data: [DONE]`.
    /// Two chunks produce two `Ok(String)` items; concatenated
    /// they reconstruct the full assistant response.
    #[tokio::test]
    async fn stream_completion_yields_chunks_in_order() {
        let server = MockServer::start().await;
        let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                        data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                        data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .mount(&server)
            .await;

        let provider = mock_provider(&server);
        let stream = provider.stream_completion(vec![]).await.unwrap();
        let items: Vec<Result<String, ProviderError>> = stream.collect().await;

        let chunks: Vec<String> = items
            .into_iter()
            .map(|i| i.expect("each item is Ok"))
            .collect();
        assert_eq!(chunks.concat(), "hello");
    }
}
