# Toolchain and build setup

This page documents the Rust toolchain choice and Windows-specific build
setup. If you're cloning this repo on a different machine you'll need to
reproduce parts of this.

## What's installed and why

| Component | Purpose | Path on this machine |
|---|---|---|
| `rustup` | Manages Rust toolchains | `~/.cargo/bin/rustup.exe` |
| Rust `gnullvm` toolchain | Compiler + std lib that link via LLVM `lld` | `~/.rustup/toolchains/stable-x86_64-pc-windows-gnullvm/` |
| llvm-mingw | The actual linker (`clang.exe`), archiver (`llvm-ar`), and Win32 import libs | `~/.toolchains/llvm-mingw-20260421-msvcrt-x86_64/` |

That's the minimum needed to build `parser.exe` on Windows without Visual
Studio.

## Why this combination, and not the obvious alternatives

We arrived here by elimination during step 1.

### Why not the rustup default (`x86_64-pc-windows-msvc`)?

It needs Visual Studio Build Tools (the "Desktop development with C++"
workload) for `link.exe`. Build Tools is multi-gigabyte, requires an
installer that's awkward to script, and we don't need any C++ from the
MSVC ecosystem. Skipped.

### Why not `x86_64-pc-windows-gnu`?

That target compiles fine but expects a real GCC-based MinGW environment
(libgcc, libgcc_eh, etc.) for linking. Installing MSYS2 + MinGW pulls in
~1.2 GB of an environment we'd otherwise never use. Tried it and got
`error: error calling dlltool 'dlltool.exe': program not found`. Skipped.

### Why `x86_64-pc-windows-gnullvm` + llvm-mingw?

`gnullvm` is a Rust target that links via Clang/LLD instead of GCC. It
pairs cleanly with [llvm-mingw](https://github.com/mstorsjo/llvm-mingw),
which is a self-contained Clang-based mingw distribution that ships as a
single ~280 MB zip — no installer, no system-wide changes.

Tradeoffs:

- **Pro**: portable, single-zip linker; no MSVC; no MinGW; statically
  links cleanly so the resulting binary has no DLL deps beyond Win32.
- **Pro**: same toolchain works for cross-compiling to ARM64, etc.
- **Con**: `gnullvm` is tier-2 host (not tier-1), so a few obscure crates
  may not be tested against it. None of our deps have hit this so far.

## How `cargo` finds the linker — `.cargo/config.toml`

```toml
[target.x86_64-pc-windows-gnullvm]
linker    = "C:/Users/omerk/.toolchains/llvm-mingw-20260421-msvcrt-x86_64/bin/x86_64-w64-mingw32-clang.exe"
ar        = "C:/Users/omerk/.toolchains/llvm-mingw-20260421-msvcrt-x86_64/bin/x86_64-w64-mingw32-llvm-ar.exe"
rustflags = ["-C", "target-feature=+crt-static"]
```

Three things going on:

1. **`linker`** — `cargo` invokes this when producing executables. Without
   this entry, `cargo` would look up `x86_64-w64-mingw32-gcc` on `PATH`,
   which is exactly the dependency we're trying to avoid.
2. **`ar`** — the archiver used to produce static libs. Same reasoning.
3. **`rustflags = ["-C", "target-feature=+crt-static"]`** — passes
   `-C target-feature=+crt-static` to `rustc`. This statically links the
   C runtime (and via llvm-mingw, libc++ / libunwind too) into the
   binary, so the resulting `parser.exe` has zero non-Win32 DLL
   dependencies. Without this flag, the binary would refuse to start on
   any machine that doesn't have llvm-mingw on `PATH`.

The paths are absolute and machine-specific. **This file should not be
committed to a public repo.** Put `.cargo/config.toml` in `.gitignore`,
or replace the paths with `${CARGO_HOME}` / environment-variable
substitution if you want it portable.

## How `parser.exe` finds the toolchain at runtime

It doesn't — it doesn't need to. `+crt-static` made the binary
self-contained at link time. You can copy `target/release/parser.exe`
(1.6 MB) to a clean Windows VM with no Rust installed and it runs.

## Cargo dependency tree

```
parser-cli 0.1.0
├── runtime deps
│   ├── serde 1.0       — derive macros for TOML deserialization
│   ├── toml 0.8        — TOML parser
│   ├── clap 4.5        — argument parsing (derive feature)
│   ├── dirs 5.0        — cross-platform home-directory lookup
│   └── url 2.5         — endpoint URL validation
└── dev deps
    └── tempfile 3      — temp dirs for unit tests
```

Total of 115 crates resolved (most are transitive). `Cargo.lock` pins
exact versions and **is committed** — for a CLI binary that's the right
default; lock files only get gitignored for libraries.

## Reproducing the setup on a fresh Windows machine

Step-by-step. ~860 MB of downloads, ~30 minutes including builds.

### 1. Install Rust

```powershell
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile $env:TEMP\rustup-init.exe
& $env:TEMP\rustup-init.exe -y --default-toolchain stable --profile default
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

The `--default-toolchain stable` here is just for getting cargo/rustup on
disk; the MSVC default toolchain it lays down won't actually be used.

### 2. Install the gnullvm toolchain

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnullvm
rustup default stable-x86_64-pc-windows-gnullvm
```

### 3. Download llvm-mingw

The latest release tag changes; the install script fetches it from the
GitHub API:

```powershell
$rel = Invoke-RestMethod -Uri "https://api.github.com/repos/mstorsjo/llvm-mingw/releases/latest" -Headers @{ "User-Agent" = "x" }
$asset = $rel.assets | Where-Object { $_.name -match 'msvcrt-x86_64\.zip$' } | Select-Object -First 1
$ProgressPreference = 'SilentlyContinue'
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $env:TEMP\llvm-mingw.zip
$dest = "$env:USERPROFILE\.toolchains"
if (-not (Test-Path $dest)) { New-Item -ItemType Directory -Path $dest | Out-Null }
Expand-Archive -Path $env:TEMP\llvm-mingw.zip -DestinationPath $dest -Force
```

After extraction the toolchain is at
`~/.toolchains/llvm-mingw-<date>-msvcrt-x86_64/`.

### 4. Update `.cargo/config.toml`

Edit the linker and `ar` paths to match the directory name from step 3
(the date suffix changes per release). Keep the `rustflags` line.

### 5. Build

```powershell
cargo build --release
```

The output is `target/release/parser.exe`.

## Build commands

| Command | What it does |
|---|---|
| `cargo build` | debug build to `target/debug/parser.exe`; ~30s clean, <2s incremental |
| `cargo build --release` | optimized build to `target/release/parser.exe`; ~40s clean |
| `cargo run -- <args>` | rebuilds debug then runs with `<args>` (use `--` to pass args through) |
| `cargo test` | runs all tests; see [05-testing.md](documentation/05-testing.md) |
| `cargo check` | type-checks without producing a binary; fastest feedback loop |
| `cargo clean` | wipes `target/`; safe, will rebuild on next invocation |
