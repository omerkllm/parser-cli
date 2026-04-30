# Cleanup and disk usage

Inventory of everything this project put on your machine, plus what's
safe to delete and how to fully uninstall.

## Quick audit (re-run anytime)

Paste into PowerShell to see current sizes:

```powershell
function Size($p) {
  if (Test-Path $p) {
    $b = (Get-ChildItem $p -Recurse -Force -EA SilentlyContinue | Measure-Object -Property Length -Sum).Sum
    "{0,8:N0} MB  {1}" -f ($b / 1MB), $p
  } else { " missing  $p" }
}
Size "D:\parser-cli\target"
Size "$env:USERPROFILE\.toolchains\llvm-mingw-20260421-msvcrt-x86_64"
Size "$env:USERPROFILE\.rustup"
Size "$env:USERPROFILE\.cargo"
Size "$env:USERPROFILE\.parser"
```

## What's on disk and what each thing is

| Location | Size today | Type | Safe to delete? |
|---|---|---|---|
| `D:/parser-cli/target/` | ~480 MB | build cache | yes — `cargo clean` regenerates in 30-40s |
| `D:/parser-cli/Cargo.lock` | <10 KB | pinned dep versions | **no** — commit this |
| `D:/parser-cli/.cargo/config.toml` | <1 KB | linker config | **no** (and don't commit; paths are machine-specific) |
| `~/.toolchains/llvm-mingw-20260421-msvcrt-x86_64/` | ~647 MB | linker + Win32 import libs | **no** while you're building Parser |
| `~/.rustup/` | ~4 GB | Rust toolchains (3 of them) | partially — see "Reclaim 2.7 GB" below |
| `~/.cargo/` | ~273 MB | crate registry + cargo binaries | not really — shared across all Rust projects |
| `~/.parser/parser.config.toml` | <1 KB | **your** real Parser config | **no** unless you want to redo `parser init` |
| `%TEMP%/rustup-init.exe` | — | installer leftover | already deleted |
| `%TEMP%/llvm-mingw.zip` | — | installer leftover | already deleted |
| Test temp dirs | — | from `tempfile::tempdir()` | auto-cleaned on test exit |

## Reclaim 2.7 GB by removing unused toolchains

When the project was set up, three Rust toolchains were installed before
`gnullvm` was settled on:

```
$ rustup toolchain list
stable-x86_64-pc-windows-gnu          ← unused now
stable-x86_64-pc-windows-gnullvm  (active, default)
stable-x86_64-pc-windows-msvc         ← unused now
```

The other two contributed nothing to the working build and can be
removed:

```powershell
rustup toolchain remove stable-x86_64-pc-windows-msvc
rustup toolchain remove stable-x86_64-pc-windows-gnu
```

Saves ~1.2 GB + ~1.5 GB = **~2.7 GB**. If you later decide to switch
back to MSVC (for example, after installing Visual Studio Build Tools),
you can reinstall it in one command.

## Periodic cleanup

### `cargo clean` — wipe the build cache

```powershell
cd D:\parser-cli
cargo clean
```

Removes `target/` (~480 MB). Next `cargo build` re-downloads/recompiles
nothing — the registry cache in `~/.cargo/registry/` is preserved — but
the build itself takes 30-40s instead of the usual <2s incremental.

Worth doing if you're low on disk space and don't plan to rebuild
imminently. Otherwise leave it alone.

### `cargo cache` cleanup (advanced, optional)

The crate registry under `~/.cargo/registry/cache` holds compressed
copies of every crate version you've ever built. If you want to trim it:

```powershell
cargo install cargo-cache    # one-time
cargo cache --autoclean      # removes old versions
```

For this project alone the savings are minimal (~50 MB at most). More
useful if you build many Rust projects.

## Test temp files

The two unit tests use `tempfile::tempdir()`, which creates a directory
under `%TEMP%` and **removes it via RAII when the `TempDir` drops at end
of test**. Nothing leaks under normal exit. An audit run after dozens of
test invocations showed zero leftover directories.

If a test process is killed mid-run (Ctrl+C, OOM), the temp dir won't be
cleaned. To check:

```powershell
Get-ChildItem $env:TEMP -Filter '.tmp*' -Directory | Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-1) }
```

Anything that shows up is safe to delete.

The two test env vars (`PARSER_TEST_KEY_INVARIANT_1`,
`PARSER_TEST_KEY_INVARIANT_2`) are set with `std::env::set_var` and only
exist in the test process — they don't persist to your shell or registry.

## Full uninstall — remove every trace of Parser and its toolchain

Run these in order. Adjust paths if you customized any.

```powershell
# 1. project files
Remove-Item -Recurse -Force D:\parser-cli

# 2. your Parser config + data
Remove-Item -Recurse -Force $env:USERPROFILE\.parser

# 3. llvm-mingw linker
Remove-Item -Recurse -Force $env:USERPROFILE\.toolchains\llvm-mingw-20260421-msvcrt-x86_64
# (if .toolchains is empty afterwards, you can remove it too)

# 4. Rust itself — if you want to uninstall Rust entirely (not just this project)
rustup self uninstall
```

The `rustup self uninstall` step removes `~/.rustup/`, `~/.cargo/`, and
the registry edits rustup made to your user `PATH`. Skip it if you use
Rust for other projects.

## What is *not* on this list

A few things you might expect but won't find:

- **No system-wide installs.** Nothing went into `Program Files`, the
  Windows registry beyond `PATH`, or any service.
- **No background processes.** Parser does not run a daemon. The future
  Indexer (step 4) will run in-process while `parser run` is active and
  exit when it does.
- **No telemetry, no network calls at idle.** Step 1 makes zero network
  requests. Step 2's provider will only call out when a `parser run`
  command is dispatched.
