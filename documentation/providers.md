# `src/providers/mod.rs` — ModelProvider trait + Message + ProviderError

50 lines. Defines how Parser talks to a model. The architectural rule:
**any OpenAI-compatible endpoint works**. The user supplies the URL,
model name, and API-key env-var name in `parser.config.toml`; the
binary stays unaware of which provider is on the other end.

Today the module contains the trait shape and a throwaway
`NoopProvider` stub. The real OpenAI-compatible HTTP implementation
lands in Step 3.

## Imports

[src/providers/mod.rs:1](src/providers/mod.rs:1):

```rust
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
```

Three imports that make the trait possible:

- `std::pin::Pin` — the `Stream` trait requires self-pinning to
  advance, so the boxed stream is wrapped in `Pin<Box<...>>`.
- `async_trait::async_trait` — the macro that rewrites the trait
  signature into something dyn-compatible.
- `futures::Stream` — the trait every async stream implements.

## `Role`

[src/providers/mod.rs:9](src/providers/mod.rs:9):

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

The `Display` impl renders each variant as the lowercase wire string
(`Role::System` → `"system"`, etc., and `Other(s)` → the raw `s`),
which is what the OpenAI-compatible chat-completions schema expects
in request bodies.

### Serialization

`Serialize` is derived. The per-variant `#[serde(rename = "...")]`
attributes mean the three known variants serialize directly to the
lowercase JSON strings the OpenAI schema requires:

- `Role::System` → JSON string `"system"`
- `Role::User` → JSON string `"user"`
- `Role::Assistant` → JSON string `"assistant"`

(`Role::Other("tool")` falls back to the default externally-tagged
form `{"Other":"tool"}` — fine for an internal decision-log dump,
but if a future vendor needs a raw-string `Other` on the wire the
real provider in Step 2 should `Display` the role explicitly when
building the request body rather than relying on the derive.)

### Deserialization

`Deserialize` is implemented manually rather than derived. The
incoming string is matched against the three known variants and
falls through to `Role::Other(s)` for anything else, so a vendor
sending `"role": "tool"` in a response round-trips cleanly without
needing the type to know about it ahead of time:

```rust
impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "system"    => Role::System,
            "user"      => Role::User,
            "assistant" => Role::Assistant,
            _           => Role::Other(s),
        })
    }
}
```

A derived `Deserialize` would produce externally-tagged JSON for
`Other` (`{"Other":"..."}`), which doesn't match how role strings
arrive on the wire. The manual impl keeps deserialization symmetric
with the renamed-variant serialization for the three known roles
and graceful for everything else.

## `Message`

[src/providers/mod.rs:30](src/providers/mod.rs:30):

```rust
#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}
```

The canonical role/content pair used everywhere a conversation turn
is represented:

- **Wire format.** Sent to the provider as part of a chat-completions
  request body. With `Serialize` derived, the struct goes straight
  through `serde_json::to_vec` without manual marshalling.
- **Agent input.** [`AgentInput::conversation_history`](agents.md#agentinput)
  is `Vec<Message>` — the same struct, imported from this module.
- **Future: decision log.** Step 7's compressor will likely store
  `Message` records too; `Deserialize` makes round-tripping free.

`content` is the message text. For Step 2 it's plain text; later
steps may carry tool-call payloads, image references, or structured
content — at which point the field shape changes here, in one place,
and consumers track the change.

The struct lives in this module rather than in `agents/` because the
provider layer is the canonical owner of wire-format concerns. The
agents module imports it (see [agents.md](agents.md#agentinput)).

## `ProviderError`

[src/providers/mod.rs:11](src/providers/mod.rs:11):

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
| `ApiError` | The endpoint returned a non-2xx response with a parsable error body. The `String` holds the body (or a useful slice of it). |
| `NetworkError` | The request never reached the endpoint, or the connection dropped. DNS failure, TLS handshake error, refused connection, timeout. |
| `AuthError` | 401 / 403 from the endpoint, or the API-key env var was missing. Surfaced separately so the CLI can suggest `parser init` or `export MY_API_KEY=...`. |
| `StreamError` | The request succeeded and a stream began, but a chunk failed to parse — malformed SSE, dropped mid-response, encoding mismatch. |

The `Display` impl writes them as `"api error: ..."`,
`"network error: ..."`, etc. `std::error::Error` is implemented so
the type widens cleanly into `Box<dyn Error>` in
[main.rs:51](src/main.rs:51) alongside `ConfigError` and
`AgentError`.

## The `ModelProvider` trait

[src/providers/mod.rs:117](src/providers/mod.rs:117):

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// The required method.
    async fn complete(
        &self,
        messages: Vec<Message>,
    ) -> Result<String, ProviderError>;

    /// Optional override. Default impl wraps complete().
    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError> {
        let response = self.complete(messages).await?;
        Ok(Box::pin(futures::stream::once(async move { response })))
    }
}
```

### `complete` vs `stream_completion`

The trait exposes two methods that return the same conceptual
result — the model's reply to a conversation — at different
granularities:

| Method | Returns | When to call | When to override |
|---|---|---|---|
| `complete` | `Result<String, ProviderError>` — the full response in one piece. | Tests, batch jobs, anywhere the caller wants the whole answer in one chunk and doesn't care about latency-to-first-token. | **Always.** This is the single required method. Every implementor must write a real `complete`. |
| `stream_completion` | `Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError>` — chunks yielded as they arrive. | The interactive CLI path, where users want to see tokens land as soon as they're decoded. | Only when the underlying transport supports streaming (OpenAI-style SSE chunks). The default implementation calls `complete` and yields the whole response as a single-item stream — semantically correct, but with none of streaming's latency or cancellation benefits. |

The split lets a minimal provider (a fixture, a mock, a non-streaming
backend) implement only `complete` and inherit a working
`stream_completion` for free, while a real OpenAI-compatible HTTP
provider overrides `stream_completion` to parse SSE deltas.

The default implementation is exactly:

```rust
let response = self.complete(messages).await?;
Ok(Box::pin(futures::stream::once(async move { response })))
```

`futures::stream::once` builds a `Stream` that yields a single
`Item` and then ends. `Box::pin` matches the trait's return
signature.

Four pieces are doing real work in the trait declaration itself:

### `#[async_trait]`

Why the macro instead of native `async fn`? The `Agent` trait gets
to use native async fn because it's only ever statically dispatched
(see [agents.md](agents.md#1-native-async-fn-in-trait--no-macro)).
`ModelProvider` is different:

1. The agent borrows the provider as `&dyn ModelProvider`. That
   requires the trait to be **dyn-compatible**.
2. The return type is `Pin<Box<dyn Stream<Item = String> + Send>>` —
   a heap-allocated, pinned, dynamically-typed stream.

Both are exactly what `#[async_trait]` was built for. Native async
fn in dyn-compatible traits is gaining ground in Rust but still has
caveats; the macro produces predictable, well-trodden code today.

The macro rewrites `async fn ...` into roughly:

```rust
fn stream_completion<'a>(
    &'a self,
    messages: Vec<Message>,
) -> Pin<Box<dyn Future<Output = Result<...>> + Send + 'a>>;
```

You don't see this — but it's why the trait is dyn-compatible.

### `Send + Sync` supertraits

A provider implementation must be safe to share across threads.
Even though Parser uses tokio's `current_thread` runtime today, the
provider may be borrowed by spawned tasks (Step 5+'s indexer is
likely to need this). The bound is cheap to enforce now; removing
it later would be a breaking change for implementors.

### `messages: Vec<Message>` taken by value

Owned because the provider often needs to mutate the conversation
(adding system prompts, formatting role markers) before sending it
on the wire. Borrowing would force every caller to clone.

### The return type

`Pin<Box<dyn Stream<Item = String> + Send>>` deserves unpacking:

- **`String`** — each item is a chunk of decoded text from the model.
  The provider is responsible for parsing the wire format (typically
  Server-Sent Events for OpenAI-compatible APIs) and yielding clean
  strings.
- **`+ Send`** — chunks must cross thread boundaries cleanly so the
  stream can be polled from a spawned task.
- **`Box<dyn Stream<...>>`** — the concrete stream type depends on
  the HTTP client library; boxing erases it so the trait signature
  stays stable across implementations.
- **`Pin<...>`** — `Stream::poll_next` requires the stream to not
  move between calls; pinning the box gives that guarantee.

## Why streaming

Tokens arrive incrementally from a model, often over hundreds of
milliseconds to many seconds. Three reasons to expose them as a
stream rather than blocking until completion:

1. **Latency to first token.** The user sees output almost
   immediately rather than after a long pause.
2. **Cancellation.** If the user kills a task, the stream can be
   dropped and the HTTP connection closed; no wasted compute or API
   credits.
3. **Compositionality.** The Compressor (Step 8+) will want to emit
   intermediate state as it works. A stream-first interface keeps
   that path simple.

The agent layer collects chunks into a single
`AgentOutput.response` today; later steps may push streaming up to
the CLI for live token display.

## `NoopProvider` — the stub

[src/providers/mod.rs:148](src/providers/mod.rs:148):

```rust
pub struct NoopProvider;

#[async_trait]
impl ModelProvider for NoopProvider {
    async fn complete(
        &self,
        _messages: Vec<Message>,
    ) -> Result<String, ProviderError> {
        Ok(String::new())
    }
}
```

A throwaway implementation that returns an empty string from
`complete`. It exists for one reason: the trait shape ships now,
but the real HTTP-talking implementation lands in the next step.
Without `NoopProvider`, [main.rs](src/main.rs) couldn't fill the
`&dyn ModelProvider` slot when calling `agent.run(...)` — the
binary wouldn't compile.

`NoopProvider` only implements `complete`. Calls to
`stream_completion` go through the trait's default implementation,
which awaits `complete` (`Ok(String::new())`) and wraps the empty
string in `futures::stream::once`. The resulting stream yields a
single empty string and then ends. That's a slight behaviour
change from the previous `futures::stream::empty()` version (which
yielded zero items) but matches the intent: "the provider has
nothing to say, but it didn't fail."

The next step deletes both the `NoopProvider` struct and its
`impl` block.

## What this module deliberately doesn't do (yet)

- **No HTTP client.** No `reqwest`, no `hyper`, no socket setup.
- **No request body construction.** OpenAI's chat-completions schema
  isn't built here; that's Step 3.
- **No SSE parsing.** Same.
- **No retries, no rate limiting, no backoff.** Same.
- **No mock provider for tests.** When tests exist that exercise the
  agent layer, a `MockProvider` will live next to the test code, not
  in the production module.
- **No structured tool-call output.** `Stream<Item = String>` is the
  current shape; if tool-use lands later, this becomes
  `Stream<Item = StreamChunk>` where `StreamChunk` is an enum of
  text/tool-call/error variants. Non-breaking change, since callers
  already have to match on what comes back.

## Step 3 preview — what the real impl will add

A new file or a new struct in this same file (TBD) will:

1. Define an `OpenAICompatibleProvider` struct holding endpoint URL,
   model name, API key, and an HTTP client (`reqwest` is the likely
   pick).
2. Provide a constructor that reads from `Config` and validates inputs.
3. Implement `stream_completion`:
   - Build a chat-completions request body with the messages plus
     `stream: true`.
   - POST to `{endpoint}/chat/completions` with the API key as a
     bearer token.
   - Parse the SSE response stream, yielding each delta's content as
     a `String`.
   - Map errors to the right `ProviderError` variant based on HTTP
     status, network failure mode, or parse error.
4. Delete `NoopProvider`.

## Cross-references

- [agents.md](agents.md) — the consumer of this trait. Specifically,
  the section on why `Agent` uses native async fn while
  `ModelProvider` uses `#[async_trait]`.
- [config.md](config.md) — where `endpoint`, `name`, `api_key`, and
  `parameters.*` come from at construction time.
- [main.md](main.md) — where `NoopProvider` gets instantiated today.
- [04-toolchain.md](04-toolchain.md) — `async-trait` and `futures` deps.
