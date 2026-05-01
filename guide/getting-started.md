# Getting started

Parser is a command-line AI coding agent: you type a task in plain
English, it reads your config, and it asks an AI model to do the
work. Today the agent is a placeholder that echoes a fixed reply —
real model responses arrive in the next release.

## Prerequisites

You need three things before you can run Parser:

1. **Rust toolchain.** Install via [rustup.rs](https://rustup.rs).
   On Windows you also need the Visual Studio Build Tools 2022
   (the rustup installer prompts for this and can install them
   for you). On Linux/macOS the toolchain installs cleanly on
   its own.
2. **An OpenRouter account.** OpenRouter is a single API that
   front-ends most major models (Anthropic, OpenAI, Meta,
   DeepSeek, Mistral, etc.). Sign up at
   [openrouter.ai](https://openrouter.ai) and create an API key
   from your account dashboard. The free tier is enough to run
   the recommended free models — see [models.md](models.md).
3. **A terminal.** PowerShell on Windows, bash/zsh on Linux/macOS,
   or any shell you are comfortable with.

## Build the binary

From the project root:

```
cargo build --release
```

This produces a single self-contained binary at
`target/release/parser` (Linux/macOS) or
`target/release/parser.exe` (Windows). It's about 1 MB and has
no runtime dependencies.

If you want to put it on your `PATH`, copy it to a directory
that's already on your `PATH` (e.g. `~/.local/bin` on Linux,
`%USERPROFILE%\bin` on Windows after adding that to `PATH`).

For the rest of this guide, the examples use the full path
`./target/release/parser` so they work without any `PATH`
changes.

## Create your config

Run the interactive setup wizard:

```
./target/release/parser init
```

It asks three questions:

1. **Provider endpoint URL.** For OpenRouter, the answer is
   `https://openrouter.ai/api/v1`. (The wizard accepts pasting
   the full chat-completions URL too — `…/api/v1/chat/completions`
   — and silently strips the suffix.)
2. **Model name.** The exact OpenRouter model identifier you
   want to use, e.g. `deepseek/deepseek-chat-v3-0324:free`. See
   [models.md](models.md) for recommended free options.
3. **API key environment variable name.** The name of the env
   var that holds your OpenRouter key. The conventional choice
   is `OPENROUTER_API_KEY`.

The wizard writes to `~/.parser/parser.config.toml` (Windows:
`C:\Users\<you>\.parser\parser.config.toml`). The write is
atomic — even if the process is killed mid-write, you never
end up with a corrupt config.

## Set your API key

Parser does **not** store your API key — it stores the *name*
of the env var, then reads the value at runtime. Set the var
in your shell:

PowerShell (Windows):

```
$env:OPENROUTER_API_KEY = "sk-or-v1-your-real-key-here"
```

bash/zsh (Linux/macOS):

```
export OPENROUTER_API_KEY="sk-or-v1-your-real-key-here"
```

Don't include surrounding quotes inside the value — Parser
rejects keys that start or end with `"`. The double quotes
above are just shell syntax delimiters.

To make the var persist across sessions, add the line to your
shell profile (`~/.bashrc`, `~/.zshrc`, or your PowerShell
profile script).

## Run your first task

```
./target/release/parser run "fix the jwt bug"
```

You'll see:

```
User: fix the jwt bug
─────────────────────────────
Response: Coder agent placeholder
─────────────────────────────
```

What the four lines mean:

- **`User: <task>`** — the task you typed, echoed back so you
  can confirm Parser received what you intended.
- **First divider** — visual separator between input and output.
- **`Response: <answer>`** — the model's reply. Right now this
  is the literal placeholder string `"Coder agent placeholder"`.
  When the real provider lands, this line shows what the model
  actually wrote.
- **Closing divider** — marks end-of-response so subsequent
  output (errors, the next prompt) is visually distinct.

You can also use the free-form short form (no `run` keyword):

```
./target/release/parser "fix the jwt bug"
```

Same output, same behaviour.

## Next

- [commands.md](commands.md) — every command and what it does.
- [models.md](models.md) — picking and switching models.
- [config.md](config.md) — full config reference.
