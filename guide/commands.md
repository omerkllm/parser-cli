# Commands

Parser has four commands today: `init`, `run`, `model-switch`,
and a free-form shorthand. Running `parser` with no arguments
prints the help text.

For all examples below, replace `parser` with the path to your
built binary if it isn't on your `PATH` (e.g. `./target/release/parser`).

---

## `parser init`

Interactive setup wizard. Use this the first time you set up
Parser or whenever you want to point it at a different
provider/model.

### Syntax

```
parser init
```

No arguments.

### What it does

1. Checks whether `~/.parser/parser.config.toml` already exists.
   If so, asks `overwrite? [y/N]`. The default is **no** — anything
   that isn't `y` or `yes` aborts cleanly without touching disk.
2. Asks three questions in order:
   - Provider endpoint URL (e.g. `https://openrouter.ai/api/v1`).
     Validated immediately as a real `http://` or `https://` URL.
     Re-prompts on empty input.
   - Model name (e.g. `deepseek/deepseek-chat-v3-0324:free`).
     Re-prompts on empty input.
   - Name of the env var that holds your API key (e.g.
     `OPENROUTER_API_KEY`). Re-prompts on empty input.
3. Creates `~/.parser/` if it doesn't exist.
4. Writes the config **atomically**: the body goes to
   `~/.parser/parser.config.toml.tmp` first, then gets renamed
   onto `parser.config.toml`. If the process is killed
   mid-write, the partial `.tmp` is removed and your old
   config (if any) is untouched.
5. Prints the path it wrote to and an `export` hint reminding
   you which env var still needs to be set.

### Example

```
$ parser init
What is your provider endpoint URL?
  example: https://openrouter.ai/api/v1
> https://openrouter.ai/api/v1
What model do you want to use?
  example: moonshotai/kimi-k2
> deepseek/deepseek-chat-v3-0324:free
What environment variable holds your API key?
  example: OPENROUTER_API_KEY
> OPENROUTER_API_KEY

wrote config to /home/you/.parser/parser.config.toml

next: set the API key environment variable so parser can read it
  export OPENROUTER_API_KEY="your-api-key-here"
```

### Possible errors

| Message starts with | Meaning | Fix |
|---|---|---|
| `endpoint ... is not a valid URL` | The URL didn't parse, or its scheme isn't `http`/`https`, or it has no host. | Re-run and paste a complete URL like `https://openrouter.ai/api/v1`. |
| `could not write ...` | The config directory or file couldn't be created. | Check permissions on your home directory. |
| `could not determine your home directory` | The `HOME` env var isn't set (rare; usually only on misconfigured Linux containers). | Set `HOME` or `USERPROFILE` and retry. |

---

## `parser run "<task>"`

Run a coding task. This is the explicit form; use it when you
want it obvious in scripts that you mean "run a task" rather
than "configure something."

### Syntax

```
parser run "<task description>"
```

The task can be one or more words. Quote it if it contains
spaces (every shell needs this).

### What it does

1. Loads `~/.parser/parser.config.toml`.
2. Validates every field — endpoint URL shape, model-name length,
   env-var-name format, the resolved API-key value (no blanks,
   no newlines, no surrounding quotes), and the numeric ranges
   for `max_tokens`, `temperature`, `context_limit`. See
   [config.md](config.md) for the full set of rules.
3. Validates the task itself:
   - It must not be empty after trimming whitespace.
   - It must not exceed 32,768 characters after trimming.
4. Currently — until the real provider lands — returns the
   literal placeholder string `"Coder agent placeholder"`.
5. Prints the formatted output.

### Example

```
$ parser run "fix the jwt bug"
User: fix the jwt bug
─────────────────────────────
Response: Coder agent placeholder
─────────────────────────────
```

### Possible errors

| Message starts with | Meaning | Fix |
|---|---|---|
| `no config found at ...` | You haven't run `parser init` yet. | Run `parser init`. |
| `environment variable ... is not set` | Your config points at an env var that doesn't exist in the current shell. | `export OPENROUTER_API_KEY=sk-or-v1-...` (or whatever name your config uses). |
| `invalid API key: value contains surrounding quotes ...` | You set the env var with literal quotes inside the value. | Re-set without inner quotes: `export KEY=value`, not `export KEY='"value"'`. |
| `invalid API key: value contains a newline ...` | You pasted a key that included a trailing newline from terminal output. | Re-export the key on a single line with no trailing newline. |
| `task cannot be empty` | The task argument was empty or only whitespace. | Provide a real task. |
| `task is N characters, maximum is 32768` | The task is too long. | Shorten it. |
| `endpoint ... is not a valid URL` | Your config has a malformed endpoint. | Edit the config, or re-run `parser init`. |
| `invalid value for ...: ...` | A field is out of range (model name too long, temperature out of `0.0..=2.0`, etc.). | Edit the config to match the rules in [config.md](config.md). |

---

## `parser model-switch`

Interactive provider/model switcher. Use this when you want to
swap which model Parser sends tasks to without editing the
config file by hand.

### Syntax

```
parser model-switch
```

No arguments.

### What it does

1. **Migrates on first run.** If your config still has the
   original flat `[model]` block (endpoint/name/api_key_env)
   and no `[[profiles]]` array, Parser silently rewrites the
   config: the existing fields move into a single `[[profiles]]`
   entry, and `model.active` gets set to that profile's name.
   The profile name is derived from the endpoint's hostname —
   `https://openrouter.ai/api/v1` becomes `openrouter`,
   `https://api.openai.com/v1` becomes `openai`, etc. After
   this one-shot rewrite, you have one profile to choose from
   plus the option to add more.
2. **Shows an arrow-key menu** of every saved profile, with a
   `+ Add new profile` row at the bottom. Each profile row
   shows the profile name aligned in a column, followed by the
   endpoint's domain in parentheses (not the full URL — just
   the host, so the line stays short and readable).
3. **On selecting a profile** — updates `model.active` in the
   config to that profile's name. The profile list itself is
   unchanged. Prints `Switched to <model> via <domain>`.
4. **On selecting `+ Add new profile`** — runs a four-question
   wizard: profile name, endpoint URL, model name, API key env
   var. Every field gets validated using the same checks
   `parser run` would apply: URL shape, trailing-slash and
   `/chat/completions` normalization, model-name length,
   env-var-name format, and the resolved API-key value (the
   env var must be set, the value must be non-blank after
   trimming, and must contain no `\n`/`\r` or surrounding
   `"`). The new profile gets appended and immediately
   activated.

### Arrow-key menu

`parser model-switch` uses the [`dialoguer`](https://crates.io/crates/dialoguer)
crate's `Select` for the menu. Controls:

- **Up / Down arrows** — move the highlight.
- **Enter** — select the highlighted entry.
- **Ctrl-C / Esc** — cancel without changing anything.

The menu opens with the highlight on the **currently active
profile** so pressing Enter without arrow-keying re-selects it
(a no-op switch).

### Adding a new profile — example

```
$ parser model-switch
? Select profile ›
  openrouter   (openrouter.ai)
❯ + Add new profile
Profile name (e.g. groq): groq
Endpoint URL: https://api.groq.com/openai/v1
Model name: llama-3.3-70b-versatile
API key env var: GROQ_API_KEY
Switched to llama-3.3-70b-versatile via api.groq.com
```

The new profile is now in your config, and `parser run` will
use it for the next task.

### Switching between existing profiles — example

```
$ parser model-switch
? Select profile ›
❯ openrouter   (openrouter.ai)
  groq         (api.groq.com)
  + Add new profile
Switched to deepseek-chat-v3-0324 via openrouter.ai
```

(The user pressed Enter to re-select `openrouter`; arrow Down
+ Enter would have switched to `groq`.)

### What gets written to the config

Before model-switch (typical first-run config):

```toml
[model]
endpoint    = "https://openrouter.ai/api/v1"
name        = "deepseek/deepseek-chat-v3-0324:free"
api_key_env = "OPENROUTER_API_KEY"
```

After running `model-switch` once and adding a Groq profile:

```toml
[model]
active = "groq"

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

The flat `endpoint`/`name`/`api_key_env` are gone — once
profiles exist, those flat fields are unused, so `model-switch`
removes them when it saves. Any `[parameters]`, `[paths]`, or
`[agents]` sections you had are preserved.

### Possible errors

| Message starts with | Meaning | Fix |
|---|---|---|
| `no config found at ...` | You haven't run `parser init` yet, so there's nothing to switch between. | Run `parser init` first. |
| `no profiles available and the existing [model] block is incomplete` | The config exists but the flat `[model]` fields are missing required entries, so migration couldn't create a profile. | Run `parser init` to write a complete config. |
| `profile `<name>` already exists` | You tried to add a new profile with a name that's already taken. | Pick a different profile name. |
| `endpoint ... is not a valid URL` | The endpoint URL you typed in the wizard doesn't parse. | Re-run and paste a complete `https://...` URL. |
| `environment variable ... is not set` | The wizard's API-key env-var validation requires the env var to actually be set in the current shell. | `export GROQ_API_KEY=...` (or whatever name you typed) and re-run. |
| `invalid value for ...` / `invalid API key: ...` | One of the wizard fields failed the same validation `parser run` applies. | Read the error — it names the field and the rule that was violated. |

---

## `parser "<task>"`

Free-form shorthand. Identical behaviour to `parser run` — use
this when you don't want to type the `run` keyword.

### Syntax

```
parser "<task description>"
```

The first argument can be anything that isn't a known command
name. Parser routes it through the same task-running code path
as `parser run`.

### Example

```
$ parser "fix the jwt bug"
User: fix the jwt bug
─────────────────────────────
Response: Coder agent placeholder
─────────────────────────────
```

### Possible errors

Identical to `parser run`. See above.

### When to prefer the explicit `run` form

In scripts, CI, or documentation — wherever you want it obvious
that you're running a task. The free-form path is for
interactive use.

---

## `parser` (no arguments)

Prints the help text and exits with code 2.

### Syntax

```
parser
```

### What it does

Lists the available subcommands and global options. Same output
as `parser --help`.

### Example

```
$ parser
AI-powered coding agent that runs in the terminal

Usage: parser [COMMAND]

Commands:
  init  Create a parser config file by answering 3 questions
  run   Run a coding task: `parser run "fix the jwt bug"`
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Other clap-driven commands

Two more commands come for free from the command-line parsing
library:

- `parser --help` — same output as bare `parser`.
- `parser --version` — prints the version from `Cargo.toml`.
- `parser help <subcommand>` — prints help for one subcommand.

These exit with code 0 (help) or 2 (no-args).

---

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The command succeeded. |
| `1` | The command ran but failed (config error, validation error, agent error). The error message goes to stderr. |
| `2` | The command line itself was malformed (no args, unknown flag, `--help`, `--version`). Help/version go to stdout. |
