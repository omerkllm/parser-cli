# `src/providers/mod.rs` — ModelProvider + OpenAIProvider + Message + ProviderError

Defines how Parser talks to a model. The architectural rule:
**any OpenAI-compatible endpoint works**. The user supplies the URL,
model name, and API-key env-var name in `parser.config.toml`; the
binary stays unaware of which provider is on the other end.

This module owns the `ModelProvider` trait, the wire-format types
(`Message`, `Role`), the error enum (`ProviderError`), and the
real `OpenAIProvider` that does HTTP and SSE.

## Imports

```rust
use std::collections::VecDeque;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
```

- `VecDeque` — buffers parsed SSE chunks the unfold closure hasn't
  yielded yet (one network packet can carry several events).
- `Pin` — the `Stream` trait requires self-pinning to advance, so
  the boxed stream is wrapped in `Pin<Box<...>>`.
- `async_trait` — rewrites the trait into a dyn-compatible shape.
- `futures::Stream`, `futures_util::StreamExt` — the trait every
  async stream implements, plus its `.next()` extension method.
- `serde_json::{json, Value}` — `json!` builds the request body,
  `Value` reads response fields without a typed schema.
- `crate::config::Config` — `OpenAIProvider::from_config` reads
  endpoint / model / api_key / max_tokens / temperature.

## `Role`

```rust
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
```

A typed enum for the `role` field of a `Message`. Using an enum
instead of a bare `String` means callers can't accidentally pass a
typo (`"asistant"`, `"User"`) — the compiler enforces correct values
for the three roles the OpenAI chat-completions schema defines.

| Variant | Wire form | When |
|---|---|---|
| `Role::System` | `"system"` | The system / instruction message at the top of a conversation. |
| `Role::User` | `"user"` | A turn from the human user. |
| `Role::Assistant` | `"assistant"` | A turn previously emitted by the model. |
| `Role::Other(String)` | the inner string | Any non-standard role a vendor may introduce — `"tool"`, `"function"`, `"developer"`, etc. The escape hatch keeps the type future-proof without baking every vendor's vocabulary into it. |

The `Display` impl renders each variant as the lowercase wire string.
`OpenAIProvider::build_body` calls `role.to_string()` rather than
`serde_json::to_value(&role)` precisely because the derived
`Serialize` produces `{"Other":"..."}` for the tuple variant — wrong
on the wire.

### Deserialization

`Deserialize` is implemented manually rather than derived. The
incoming string is matched against the three known variants and
falls through to `Role::Other(s)` for anything else, so a vendor
sending `"role": "tool"` in a response round-trips cleanly.

## `Message`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}
```

The canonical role/content pair used everywhere a conversation turn
is represented:

- **Wire format.** Sent to the provider as part of a chat-completions
  request body. `OpenAIProvider::build_body` re-serializes via
  `role.to_string()` to dodge the `Other` problem above.
- **Agent input.** [`AgentInput::conversation_history`](agents.md#agentinput)
  is `Vec<Message>` — the same struct, imported from this module.

## `ProviderError`

```rust
#[derive(Debug)]
pub enum ProviderError {
    ApiError(String),
    NetworkError(String),
    AuthError(String),
    StreamError(String),
}
```

Each variant carries a `String` message specific enough to copy-paste
into a bug report:

| Variant | When it fires |
|---|---|
| `ApiError` | The endpoint returned a non-2xx response other than 401. The string is either a tailored message (402: insufficient credits; 429: rate limited) or `"HTTP {status}: {body}"` for everything else. |
| `NetworkError` | The request never reached the endpoint, or the byte-stream dropped mid-response: DNS failure, TLS handshake error, refused connection, timeout. |
| `AuthError` | 401 from the endpoint. Surfaced separately so the CLI can suggest `parser init` to reconfigure rather than dumping a raw body. |
| `StreamError` | A streaming SSE chunk arrived but the `data: …` JSON couldn't be parsed (`malformed SSE chunk: ...`). |

The `Display` impl writes them as `"api error: ..."`,
`"network error: ..."`, etc. `std::error::Error` is implemented so
the type widens cleanly into `Box<dyn Error>` in
[main.rs](../src/main.rs) alongside `ConfigError` and `AgentError`.

## The `ModelProvider` trait

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(
        &self,
        messages: Vec<Message>,
    ) -> Result<String, ProviderError>;

    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>,
        ProviderError,
    > {
        let response = self.complete(messages).await?;
        Ok(Box::pin(futures::stream::once(async move { Ok(response) })))
    }
}
```

### Caller owns the system message

The provider serializes whatever `Vec<Message>` it receives —
nothing is added implicitly. Callers (today: `main.rs` and
`CoderAgent::run`) are responsible for prepending a `Role::System`
turn if they want one. This keeps the trait contract narrow: a
provider is a transport, not a prompt-injection layer.

### `complete` vs `stream_completion`

| Method | Returns | When |
|---|---|---|
| `complete` | `Result<String, ProviderError>` — the full response in one piece. | Tests, batch jobs, any caller that wants the whole answer at once. |
| `stream_completion` | `Result<Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>, ProviderError>` — chunks yielded as they arrive. | The interactive CLI path. |

The split lets a minimal provider (a fixture, a mock, a non-streaming
backend) implement only `complete` and inherit a working
`stream_completion` for free.

### Why `Stream<Item = Result<String, ProviderError>>` and not `Item = String`

A streaming SSE response can fail mid-flight in two ways the caller
needs to distinguish:

1. **Malformed JSON in a `data:` line.** The provider yields
   `Err(ProviderError::StreamError(...))`. The caller decides
   whether to abort, log and continue, or surface a partial response.
2. **The byte stream itself drops.** The provider yields
   `Err(ProviderError::NetworkError(...))` and ends.

If items were bare `String`, both cases would be indistinguishable
from a clean end-of-stream — the user would silently lose data. The
`main.rs` loop uses this distinction to print
`"Stream interrupted. Partial response shown above."` when bytes
had already streamed before the error.

### The `Send + Sync` supertraits

A provider implementation must be safe to share across threads. Even
though Parser uses tokio's `current_thread` runtime today, the
provider may be borrowed by spawned tasks (the indexer in a later
step is likely to need this).

## `OpenAIProvider` — the real implementation

```rust
pub struct OpenAIProvider {
    endpoint: String,
    model: String,
    api_key: String,
    max_tokens: u32,
    temperature: f32,
    client: reqwest::Client,
}
```

Works with any endpoint that speaks the OpenAI chat-completions
wire format: OpenRouter, OpenAI itself, Groq, Together AI, Ollama,
LM Studio, and others. The five fields are copied from the
validated [`Config`](config.md) at startup.

### Construction

```rust
pub fn from_config(cfg: &Config) -> Self
```

Cheap; call once per process. `reqwest::Client::new()` is the
default client — no custom timeout, no retries, no proxy
configuration. Those land if and when they're needed.

### Request body

```json
{
  "model": "{model}",
  "messages": [...],
  "stream": false,
  "max_tokens": {max_tokens},
  "temperature": {temperature}
}
```

`stream` is the only difference between the body sent by `complete`
(false) and `stream_completion` (true). Everything else is shared
via the private `build_body` helper.

### Headers

| Header | Value | Purpose |
|---|---|---|
| `Authorization` | `Bearer {api_key}` | Standard chat-completions auth. |
| `Content-Type` | `application/json` | What the body is. |
| `HTTP-Referer` | `https://github.com/omerkllm/parser-cli` | Identifies the calling app. **OpenRouter-specific** — used to rank the app in their public leaderboard. Optional in the OpenAI spec; recommended for any tool talking to OpenRouter. |
| `X-Title` | `parser-cli` | Same as above — OpenRouter shows this as the app name in user-visible dashboards. |

### Status mapping

The mapping is identical for `complete` and `stream_completion`'s
pre-stream status check:

| Status | Variant | Message |
|---|---|---|
| 200 | (parsed) | — |
| 401 | `AuthError` | `"invalid API key — run parser init to reconfigure"` |
| 402 | `ApiError` | `"insufficient credits — add credits at openrouter.ai/credits"` |
| 429 | `ApiError` | `"rate limited — wait a moment and try again"` |
| other 4xx/5xx | `ApiError` | `"HTTP {status}: {body}"` (body fetched via `response.text()`) |
| send error | `NetworkError` | `e.to_string()` from reqwest |

### `complete` — the blocking path

POSTs the request, parses the response as `serde_json::Value`,
walks `choices[0].message.content` and returns it as a `String`.
A response with the right status but a missing field returns
`ApiError("response missing choices[0].message.content")`.

### `stream_completion` — the SSE path

After the same pre-flight status check, the response body is read
via `response.bytes_stream()`. The byte stream is wrapped in
`futures_util::stream::unfold` with four pieces of state threaded
through (a tuple — type inferred so we never have to name
`bytes::Bytes`):

1. **The pinned byte stream itself.** `Box::pin` makes it `Unpin`,
   which lets the unfold closure call `.next().await` on it
   without manual pin-projection.
2. **A line buffer (`String`).** Bytes that arrived without a
   trailing `\n` yet — the next chunk completes them.
3. **A `VecDeque<Result<String, ProviderError>>` of pending items.**
   One network chunk can contain multiple SSE events; we buffer
   them and drain one per unfold step.
4. **A `done` flag.** Set when we hit `data: [DONE]` so the next
   poll ends the stream.

The line-parser strips `\r\n`, ignores empty lines and SSE
comments (lines starting with `:`), recognizes `data: [DONE]` as
the end-of-stream sentinel, and parses every other `data: <json>`
line into a `Value`. From the parsed JSON, it walks
`choices[0].delta.content` and yields it as `Ok(String)` if
non-null, non-empty.

JSON parse failures emit `Err(ProviderError::StreamError(...))`;
byte-stream failures from reqwest emit
`Err(ProviderError::NetworkError(...))` and end the stream.

## Tests

`#[cfg(test)] mod tests` uses [`wiremock`](https://docs.rs/wiremock)
to spin up a local HTTP server and verify the four critical paths.

| Test | Proves |
|---|---|
| `complete_returns_content_on_200` | A 200 response with a well-formed body yields the inner `choices[0].message.content` string. |
| `complete_returns_auth_error_on_401` | A 401 maps to `ProviderError::AuthError`, not `ApiError` — the CLI's "run parser init" suggestion depends on this. |
| `complete_returns_api_error_on_429` | A 429 maps to `ApiError` carrying the `"rate limited"` message. |
| `stream_completion_yields_chunks_in_order` | An SSE body with two `data:` chunks plus `[DONE]` yields two `Ok(String)` items in order; concatenated they reconstruct the full assistant response. |

The tests build an `OpenAIProvider` directly with the wiremock
URI — `from_config` is bypassed because the tests don't need a
full `Config`.

## What this module deliberately doesn't do

- **No retries, no rate-limiting, no exponential backoff.** A 429
  fails fast with a useful message; orchestration logic for
  retrying lives at a higher layer.
- **No timeout on the reqwest client.** Defaults apply.
- **No tool-calling, no function-calling, no JSON mode.** Plain
  text in, plain text out.
- **No image / vision support.** `Message.content` is `String`.
- **No structured tool-call output in the stream.** Items are
  text deltas only; if tool-use lands later, the `Stream<Item>`
  type widens to an enum.

## Cross-references

- [agents.md](agents.md) — the consumer of this trait.
- [config.md](config.md) — where every field on `OpenAIProvider`
  comes from.
- [main.md](main.md) — where `OpenAIProvider::from_config` and
  `stream_completion` get called for the live CLI path.
