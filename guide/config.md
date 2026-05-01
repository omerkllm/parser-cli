# Config file reference

Parser reads its configuration from a single TOML file. This page
documents every field, every default, every validation rule, and
where the file lives on each platform.

For deeper internals (the two-layer loader, the `ConfigError`
variants), see [`documentation/config.md`](../documentation/config.md).

## Where the config file lives

| Platform | Path |
|---|---|
| Windows | `C:\Users\<you>\.parser\parser.config.toml` |
| Linux | `/home/<you>/.parser/parser.config.toml` |
| macOS | `/Users/<you>/.parser/parser.config.toml` |

The path is computed via the standard `dirs::home_dir()` lookup,
so anything that affects "what is my home directory" (e.g. the
`HOME` env var on Unix, `USERPROFILE` on Windows) flows through.

## What happens if the config file is missing

`parser run` (and the free-form shorthand) fails with:

```
error: no config found at <path>
  run `parser init` to create one
```

Exit code is `1`. The message tells you exactly what to do:
run `parser init` to walk through the setup wizard.

`parser init` itself doesn't need an existing config — it's the
command you use to create one.

## Minimal config

Three required fields. All under the `[model]` section. This is
exactly what `parser init` writes:

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"
```

That's enough to run Parser. Every other field has a default.

## Full config

Every optional field set to a non-default value:

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"

[parameters]
max_tokens     = 4096
temperature    = 0.7
context_limit  = 128000

[paths]
data_dir           = "~/.parser"
workspace_data_dir = ".parser"

[agents]
planner_model    = "anthropic/claude-opus-4"
coder_model      = "deepseek/deepseek-chat-v3-0324:free"
critic_model     = "openai/gpt-5"
debugger_model   = "openai/gpt-5"
compressor_model = "deepseek/deepseek-chat-v3-0324:free"
```

You only need to include the sections you actually want to
override. A config with just `[model]` works just as well —
everything else falls back to defaults.

## Field-by-field reference

### `[model]` — required

The exact required-field set depends on whether you're using
**profiles** (see below) or the original flat schema. Every
config has a `[model]` section either way.

| Field | Type | Required when | Description |
|---|---|---|---|
| `active` | string | profiles are in use | The `name` of one entry in the `[[profiles]]` array. Tells `parser run` which profile's fields to load. Set automatically by `parser model-switch`. |
| `endpoint` | string | no profiles defined | Base URL of an OpenAI-compatible chat-completions API. Must be a valid `http://` or `https://` URL with a host. A trailing `/` is silently stripped, and a trailing `/chat/completions` is also silently stripped (so pasting the full chat-completions URL works). |
| `name` | string | no profiles defined | The model identifier the endpoint expects, e.g. `deepseek/deepseek-chat-v3-0324:free`. Cannot be empty, only-whitespace, or longer than 200 characters. |
| `api_key_env` | string | no profiles defined | The *name* of the env var that holds your API key. Cannot contain whitespace and cannot exceed 200 characters. Parser reads the value of this env var at startup and uses it to authenticate. |

If `[[profiles]]` entries exist, the `active`/`endpoint`/
`name`/`api_key_env` rule reverses: `active` is required (and
must match a profile name), and the three flat fields are
ignored — they're not even kept in the file after
`parser model-switch` saves.

If `[[profiles]]` is missing or empty, Parser falls back to the
flat fields exactly as before. This is the backward-compatible
path: configs written before profiles existed still work
without modification.

### `[[profiles]]` — optional

An array of profile entries. Each entry is a full bundle of
provider/model/api-key fields. Activating a profile is just a
matter of setting `model.active` to the profile's `name`.

```toml
[[profiles]]
name = "openrouter"
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"
```

| Field | Type | Description |
|---|---|---|
| `name` | string | The profile's identifier — what you reference from `model.active`. Choose anything: `openrouter`, `groq-free`, `local-ollama`. Must be unique within the profile array. |
| `endpoint` | string | Same shape and rules as the flat `[model].endpoint` (validated URL, trailing-slash and `/chat/completions` stripping). |
| `model` | string | Same shape and rules as the flat `[model].name` (≤ 200 characters, non-empty). Note the field is named `model` here, not `name` — the profile's own `name` is the profile identifier. |
| `api_key_env` | string | Same shape and rules as the flat `[model].api_key_env` (no whitespace, ≤ 200 characters). The named env var must be set when `parser run` (or `parser model-switch` adding this profile) is called. |

### Backward-compatibility migration

The first time you run `parser model-switch` on a config that
still uses the flat schema, Parser automatically rewrites the
file in profile form: the existing `[model].endpoint`/`name`/
`api_key_env` get moved into a single `[[profiles]]` entry, and
`model.active` is set to that profile's name.

The migrated profile's name is derived from the endpoint
hostname:

| Endpoint | Migrated profile name |
|---|---|
| `https://openrouter.ai/api/v1` | `openrouter` |
| `https://api.openai.com/v1` | `openai` |
| `https://api.groq.com/openai/v1` | `groq` |
| `https://api.together.xyz/v1` | `together` |
| anything that doesn't parse | `default` |

After the migration, your config looks like this — endpoint/
name/api_key_env are gone from `[model]`, replaced by `active`
plus a profile entry:

```toml
[model]
active = "openrouter"

[[profiles]]
name = "openrouter"
endpoint = "https://openrouter.ai/api/v1"
model = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"
```

You can rename the profile by editing the file directly — both
`name` in the profile and `active` under `[model]` need to
match.

### `[parameters]` — optional

| Field | Type | Default | Description |
|---|---|---|---|
| `max_tokens` | integer | `4096` | Maximum tokens the model is allowed to generate per response. Must be in the range `1..=32768`. |
| `temperature` | float | `0.7` | Sampling temperature. `0.0` is deterministic, higher values are more random. Must be in the range `0.0..=2.0`. |
| `context_limit` | integer | not set | Total token budget for the conversation (history + new task + response). Optional — leave it out to let the model's own limit apply. When set, must be in `1..=2_000_000` and must be **strictly greater than `max_tokens`** (a context window equal to or smaller than the output cap leaves no room for the input). |

### `[paths]` — optional

| Field | Type | Default | Description |
|---|---|---|---|
| `data_dir` | string | `~/.parser` | Where Parser stores its persistent data (cache, decision log — both forthcoming). The leading `~` expands to your home directory at load time, on every platform, so the resolved path is always absolute. |
| `workspace_data_dir` | string | `.parser` | Where Parser stores per-workspace data. Resolved relative to the current working directory by convention, so each project can have its own. |

### `[agents]` — optional

When the multi-agent system lands, each role can use a different
model. Today these fields are reserved — the loader accepts them
but nothing reads them yet. Each defaults to whatever you set
for `model.name`.

| Field | Type | Default | Description |
|---|---|---|---|
| `planner_model` | string | `model.name` | Model used by the Planner agent. |
| `coder_model` | string | `model.name` | Model used by the Coder agent. |
| `critic_model` | string | `model.name` | Model used by the Critic agent. |
| `debugger_model` | string | `model.name` | Model used by the Debugger agent. |
| `compressor_model` | string | `model.name` | Model used by the Compressor agent. |

## Validation rules

Parser validates every field at load time. If anything is wrong
you get a clear error message and exit code `1` — no half-loaded
config ever reaches the rest of the program.

### Endpoint

- **Must be present.** Empty / missing → "required field
  `model.endpoint` is missing".
- **Must be a valid URL.** Parsed via the same library a browser
  uses. Bad shape → "endpoint `…` is not a valid URL: …".
- **Scheme must be `http` or `https`.** Anything else → "scheme
  `…` is not http or https".
- **Must have a host.** `http:///path` is rejected with "missing
  host".
- **Trailing `/` is stripped silently.** `https://x/api/v1/` →
  `https://x/api/v1`. No error, just normalization.
- **Trailing `/chat/completions` is stripped silently.**
  `https://api.openai.com/v1/chat/completions` →
  `https://api.openai.com/v1`. Saves you from double-pathing.

### Model name

- **Must be present and non-empty after trimming whitespace.**
  Empty / whitespace-only → "required field `model.name` is
  missing", or in isolation → "must not be empty or contain
  only whitespace".
- **Must not exceed 200 characters.** Beyond that → "name is N
  characters, maximum is 200".

### API-key env-var name

- **Must be present and non-empty after trimming.** Empty →
  "required field `model.api_key_env` is missing".
- **Must contain no whitespace.** Names with spaces can't be
  set via standard shell syntax anyway. → "must not contain
  whitespace".
- **Must not exceed 200 characters.** → "name is N characters,
  maximum is 200".

### Resolved API-key value

These checks run after Parser reads the env var that
`api_key_env` points at.

- **Env var must be set.** Otherwise → "environment variable
  `…` is not set" with an `export …` hint.
- **Value must not be empty after trimming.** → "value is
  empty after trimming whitespace".
- **Value must contain no `\n` or `\r`.** Catches the most
  common copy-paste mistake (trailing newline from terminal
  output). → "value contains a newline or carriage return —
  common copy-paste mistake; re-export the variable on a
  single line".
- **Value must not start or end with a `"` character.** Catches
  the Windows / PowerShell mistake of running
  `set KEY="value"` (which stores the quotes literally). →
  "value contains surrounding quotes — set the key without
  quotes: export KEY=value".

### Parameters

- **`temperature`** must be in `0.0..=2.0`. Out of range →
  "invalid value for `parameters.temperature`: N is outside
  the allowed range 0..=2".
- **`max_tokens`** must be in `1..=32768`. Out of range →
  "invalid value for `parameters.max_tokens`: N is outside
  the allowed range 1..=32768".
- **`context_limit`** when set must be in `1..=2_000_000`. Out
  of range → "invalid value for `parameters.context_limit`: N
  is outside the allowed range 1..=2000000".
- **`context_limit`** when set must be strictly greater than
  `max_tokens`. Equal or smaller → "must be greater than
  max_tokens (N)".

### Paths

`~/...` paths are expanded to absolute paths at load time, on
every platform. Paths without a leading `~` are taken literally.
No validation beyond that — Parser doesn't try to create or
verify the directories until it actually needs them.

## What happens after validation

Once validation passes, Parser holds a `Config` struct in
memory with every field guaranteed present and well-formed.
The rest of the program reads from this struct and never
re-validates anything. In particular, `model.api_key` holds
the **resolved** API key value (not the env-var name) — the
provider layer can use it directly.
