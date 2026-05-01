# Models

Parser is provider-agnostic — it doesn't care which model you
use, as long as the endpoint speaks the OpenAI chat-completions
format. OpenRouter is the recommended provider because it
front-ends almost every major model under one API.

This page covers how to pick a model, how to switch models, and
which free OpenRouter models are worth trying.

## How the model is configured

Parser reads two related fields from `~/.parser/parser.config.toml`:

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"
```

- **`endpoint`** — the base URL of an OpenAI-compatible API.
  For OpenRouter this is `https://openrouter.ai/api/v1`. Other
  providers (Ollama, Groq, Together AI, LM Studio) work too;
  set this to whatever URL their docs publish.
- **`name`** — the exact model identifier the endpoint expects.
  Different providers use different naming conventions:
  OpenRouter uses `<vendor>/<model>:<tier>`, Ollama uses bare
  names like `llama3.3`, etc.
- **`api_key_env`** — the *name* of the environment variable
  that holds your API key. Parser never stores the key value
  itself.

See [config.md](config.md) for the full list of fields and
validation rules.

## How to change the model

Three options. Pick whichever you find easier for the situation.

### Option 1: `parser model-switch` — recommended

```
parser model-switch
```

This is the fastest way to switch between providers/models you
use regularly. It's an interactive arrow-key menu of every
**profile** you've saved (more on profiles below). Pick one and
hit Enter — done. Or pick `+ Add new profile` from the bottom
of the menu to add a new provider/model combination on the fly.

See [commands.md → `parser model-switch`](commands.md#parser-model-switch)
for the full walkthrough, including a sample session showing
both the switch path and the add-new-profile path.

### Option 2: edit the config directly

Open `~/.parser/parser.config.toml` in any text editor and
change the `model` line of whichever profile is active (or the
flat `name` line if your config doesn't use profiles yet):

```toml
[model]
active = "openrouter"

[[profiles]]
name = "openrouter"
endpoint = "https://openrouter.ai/api/v1"
model = "meta-llama/llama-3.3-70b-instruct:free"   # ← edited
api_key_env = "OPENROUTER_API_KEY"
```

Save the file. The next `parser run` call picks up the new
model immediately — Parser reads the config fresh on every
invocation.

### Option 3: re-run `parser init`

```
parser init
```

It detects the existing config and asks `overwrite? [y/N]`.
Answer `y`, then walk through the three questions again with
the new model name. **Caveat:** `parser init` rewrites the
whole config from scratch, which means any saved profiles get
wiped. Use `parser model-switch` instead if you want to keep
them.

## Profiles

A **profile** is a named bundle of three fields:

| Field | What it is |
|---|---|
| `endpoint` | The provider's base URL (`https://openrouter.ai/api/v1`, `https://api.groq.com/openai/v1`, etc.). |
| `model` | The model identifier the provider expects (`deepseek/deepseek-chat-v3-0324:free`, `llama-3.3-70b-versatile`, etc.). |
| `api_key_env` | The env var that holds the API key for that provider (`OPENROUTER_API_KEY`, `GROQ_API_KEY`, etc.). |

Profiles let you keep multiple provider configurations side by
side in one config file and flip between them with one command.
Typical setups people use:

- **Free tier vs paid tier on the same provider** — `openrouter-free`
  with a free model, `openrouter-paid` with `anthropic/claude-opus-4`.
- **Cloud vs local** — `openrouter` for the cloud, `ollama` for
  the local model running on `localhost:11434`.
- **Per-task model preference** — `coding` with DeepSeek,
  `reasoning` with Llama-3.3-70B, `quick` with Mistral-7B.

The active profile is recorded in the `model.active` field of
your config:

```toml
[model]
active = "openrouter"

[[profiles]]
name = "openrouter"
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"

[[profiles]]
name = "groq"
endpoint = "https://api.groq.com/openai/v1"
model = "llama-3.3-70b-versatile"
api_key_env = "GROQ_API_KEY"
```

`parser run` reads `model.active`, looks up the matching
`[[profiles]]` entry, and uses its three fields. If you don't
have any `[[profiles]]` (e.g. you've just run `parser init` and
haven't run `parser model-switch` yet), Parser falls back to
the flat `[model]` fields — fully backward-compatible with
configs written before profiles existed.

The first time you run `parser model-switch` on a flat config,
it migrates the existing fields into a single profile
automatically. You don't need to do anything.

See [config.md → Profiles](config.md#profiles) for the exact
TOML schema and validation rules.

## Recommended free OpenRouter models

OpenRouter offers a handful of models marked with the `:free`
suffix that don't consume credits. The trade-off is rate
limits and occasional capacity-based queueing, but for
exploring Parser they're plenty.

| Model identifier | Best for |
|---|---|
| `deepseek/deepseek-chat-v3-0324:free` | **Coding.** Strong on code generation, refactoring, and bug-fix tasks. The default choice for Parser. |
| `meta-llama/llama-3.3-70b-instruct:free` | **Reasoning.** Better at multi-step logical problems and longer chains of thought. Use when the task needs explanation, not just code. |
| `google/gemma-3-27b-it:free` | **Instruction following.** Sticks closely to what you ask for, with less drift. Good when you have a precise prompt and want a precise answer. |
| `mistralai/mistral-7b-instruct:free` | **Lightweight and fast.** Smallest of the four, fastest first-token latency. Good for quick iterations or short tasks where a 70B model is overkill. |

To use any of these, paste the identifier into your config's
`name` field exactly as written above (including the `:free`
suffix).

OpenRouter's model catalog at
[openrouter.ai/models](https://openrouter.ai/models) lists
every available model — filter by "Pricing: Free" to see the
current free tier.

## What about paid models?

Anything OpenRouter offers works the same way. Examples:

- `anthropic/claude-opus-4` — top-tier reasoning, paid.
- `openai/gpt-5` — paid.
- `moonshotai/kimi-k2` — strong coder, paid.

Just paste the identifier into your config. Make sure your
OpenRouter account has credits or a payment method.

## What about other providers?

Parser doesn't lock you to OpenRouter. To use a different
provider, change the `endpoint` (and likely `name` and
`api_key_env`) in your config:

| Provider | Endpoint | Notes |
|---|---|---|
| **Ollama** (local) | `http://localhost:11434/v1` | Run `ollama serve` first; `name` is the local model name like `llama3.3`. No API key needed but `api_key_env` still has to point at *some* env var (set it to anything). |
| **Groq** | `https://api.groq.com/openai/v1` | `name` like `llama-3.3-70b-versatile`. |
| **Together AI** | `https://api.together.xyz/v1` | `name` like `meta-llama/Llama-3.3-70B-Instruct-Turbo`. |
| **LM Studio** (local) | `http://localhost:1234/v1` | Whatever model you've loaded in LM Studio. |

Any OpenAI-compatible endpoint works. Parser only cares that
the endpoint accepts a POST to `/chat/completions` with the
standard request shape.

