# Build setup — Cargo.toml, toolchain, release profile, .cargo/config.toml

Everything a developer needs to build Parser from source: dependencies,
the toolchain choice, the release-profile flags, and the per-machine
linker config. If you're cloning the repo on a new Windows machine,
this file plus the Quickstart in [README.md](README.md) is enough to
reproduce the build.

## `Cargo.toml`

The full file:

```toml
[package]
name = "parser-cli"
version = "0.1.0"
edition = "2021"
description = "AI-powered coding agent that runs in the terminal"

[[bin]]
name = "parser"
path = "src/main.rs"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
clap = { version = "4.5", features = ["derive"] }
dirs = "5.0"
url = "2.5"
tokio = { version = "1", features = ["macros", "rt"] }
async-trait = "0.1"
futures = "0.3"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = "abort"

[dev-dependencies]
tempfile = "3"
```

### Package name vs binary name

The `[package]` name is `parser-cli` (Cargo crate naming) but the
binary is `parser` (what the user types). The `[[bin]]` section pins
that explicitly so `cargo build` produces `target/<profile>/parser.exe`,
not `parser-cli.exe`.

### Runtime dependencies

| Crate | Version | Why |
|---|---|---|
| `serde` | 1.0 (`derive`) | `#[derive(Deserialize)]` for the raw config structs in [config.md](config.md). |
| `toml` | 0.8 | Parser reads its config as TOML. |
| `clap` | 4.5 (`derive`) | CLI argument parsing. The derive feature is what makes `#[derive(Parser)]` work — see [main.md](main.md). |
| `dirs` | 5.0 | Cross-platform `home_dir()`. Used by [`config::home_dir`](config.md). |
| `url` | 2.5 | Endpoint URL validation in [`config::validate_endpoint`](config.md#validation-rules). |
| `tokio` | 1 (`macros`, `rt`) | Async runtime. `macros` enables `#[tokio::main]`; `rt` is the single-threaded runtime — see [main.md](main.md#async-runtime). |
| `async-trait` | 0.1 | Macro that rewrites async fns in dyn-compatible traits. Used by `ModelProvider` only — see [providers.md](providers.md#async_trait). |
| `futures` | 0.3 | The `Stream` trait used in `ModelProvider::stream_completion`'s return type, plus `futures::stream::empty()` for `NoopProvider`. |

### Dev dependencies

| Crate | Version | Why |
|---|---|---|
| `tempfile` | 3 | RAII temp directories for the unit tests in [testing.md](testing.md). |

### `Cargo.lock`

Pins exact versions of all 130 transitively-resolved crates. Committed
to the repo — for a CLI binary that's the right default; lock files
only get gitignored for libraries.

To see the full tree:

```powershell
cargo tree
```

## Release profile

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

Five flags. Each one trades compile time for binary size, runtime
speed, or both.

| Flag | Effect | Trade-off |
|---|---|---|
| `opt-level = 3` | Maximum compiler optimization. Inlining, loop unrolling, vectorization, etc. | Slower compile. The default for `--release` is already `3`, but we set it explicitly so the value is auditable in the file. |
| `lto = true` | **Link-time optimization** across all 130 crates. Inlines and dead-code-eliminates *across crate boundaries* — without LTO each crate is optimized in isolation, leaving inlining opportunities on the table. | Significantly slower release build (~45s cold vs ~15s without LTO). |
| `codegen-units = 1` | Compile the entire crate as a single codegen unit instead of splitting into ~16 parallel units. Single unit gives the optimizer **full visibility** of the whole crate at once, enabling more aggressive inlining. | No parallelism within the crate during codegen — slower release build. Has no effect on incremental compile time of dependencies. |
| `strip = true` | Removes debug symbols from the release binary. | Crashes from a stripped binary have no symbol information — debugging a release-only crash requires building unstripped first. For a small CLI this is fine. |
| `panic = "abort"` | A `panic!` aborts the process immediately instead of unwinding the stack. Removes the entire **stack-unwinding machinery** from the binary — destructors aren't run, `catch_unwind` doesn't work. | A panic in a CLI process is a fatal error anyway; we want the process gone, not a graceful unwind. The library-grade alternative (`panic = "unwind"`) is dead weight here. |

### Effect on this binary

Measured on `target/release/parser.exe` after the latest build:

| Metric | Value |
|---|---|
| Binary size | **0.99 MB** |
| Peak RAM (`parser run hello`, parallel-sampled across 200 runs) | **9.66 MB** |
| Cold release build | ~45 s |

The 0.99 MB binary is what the five profile flags produce together.
Without the flags, the same build is ~1.7 MB — `strip` removes ~200
KB of debug symbols, `lto` + `codegen-units = 1` delete dead code
across crate boundaries, `panic = "abort"` removes unwinding
machinery.

RAM is small because the binary's working set is small (load a TOML,
print four lines, exit). The optimizations cut binary size and
startup time, not steady-state memory.

## Toolchain — MSVC + Visual Studio Build Tools 2022

The Rust target is **`x86_64-pc-windows-msvc`** — the rustup default.
Linking is done by `link.exe` from the Visual Studio 2022 Build
Tools, which also provides the MSVC C/C++ compiler and the Windows
SDK.

### What's installed

| Component | Path |
|---|---|
| `rustup` | `~/.cargo/bin/rustup.exe` |
| Rust MSVC toolchain | `~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/` |
| Visual Studio Build Tools 2022 (VCTools workload) | `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\` |
| Windows 11 SDK 10.0.26100 | `C:\Program Files (x86)\Windows Kits\10\` |

The MSVC linker (`link.exe`), C/C++ compiler (`cl.exe`), and the
static C runtime libraries live under the VS Build Tools tree. The
Windows SDK provides headers (`<windows.h>`, `<winnt.h>`) and import
libraries (`kernel32.lib`, `user32.lib`, etc.).

### Why MSVC

- **It's the rustup default.** Tier-1 host platform; every crate in
  the ecosystem is tested against it. No surprises with obscure
  dependencies.
- **Standard, well-documented setup.** Known good with all developer
  tooling (debuggers, profilers, IDE integrations) without bridging.
- **No third-party linker.** `link.exe` is vendored by Microsoft and
  doesn't need a separate download or `linker = "..."` entry in
  `.cargo/config.toml`.

The trade-off is install size: ~5 GB on disk between Build Tools
and the Windows SDK, vs. a 280 MB self-contained zip for the
alternative. For a development machine this is acceptable.

### Historical note: gnullvm + llvm-mingw

Earlier in the project, `x86_64-pc-windows-gnullvm` paired with the
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw) toolchain was
used as the local build setup. It was attractive because it shipped
as a single ~280 MB zip with no installer, and produced a
self-contained binary via `+crt-static` without any Microsoft
runtime dependency.

It was replaced by MSVC for the reasons above (tier-1 status,
ecosystem alignment, no third-party linker). The migration left no
gnullvm artifacts in the repo: `.cargo/config.toml` now references
only `[target.x86_64-pc-windows-msvc]`, and `~/.toolchains/` no
longer contains llvm-mingw.

If you ever need to revert: `rustup default stable-x86_64-pc-windows-gnullvm`,
re-extract llvm-mingw under `~/.toolchains/`, and put the linker /
ar entries back in `.cargo/config.toml`. There's no hard reason to.

## `.cargo/config.toml`

This file is **gitignored** because the contents are machine-specific
(absolute paths to the MSVC install). Its contents:

```toml
[target.x86_64-pc-windows-msvc]
linker    = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe"
rustflags = ["-C", "target-feature=+crt-static"]

[env]
INCLUDE = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/ucrt;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/shared;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/um;C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/winrt"
LIB     = "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/lib/x64;C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/ucrt/x64;C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64"
```

Three things going on:

- **`linker = "..."`** — points cargo at `link.exe` directly. On a
  cleanly-installed VS Build Tools, cargo's `cc-rs` discovers the
  linker automatically by running `vswhere`. On this machine the VS
  installer left the instance metadata partially blank
  (`isComplete: ""` in `state.json`), so `vswhere` filters it out
  with default flags and `cc-rs` returns no MSVC found. Pinning the
  linker path bypasses that discovery.
- **`[env]` section** — sets `INCLUDE` and `LIB` for every process
  cargo spawns (rustc, build scripts, `link.exe`). These are the
  same values `vcvars64.bat` would set, but persisted in the project
  config rather than requiring a developer to source the bat file in
  every shell.
- **`rustflags = ["-C", "target-feature=+crt-static"]`** — passes
  `-C target-feature=+crt-static` to `rustc`. **This statically
  links the Microsoft Visual C++ runtime** (`vcruntime140.dll`,
  `msvcp140.dll`, the UCRT) into the binary, so the resulting
  `parser.exe` runs on Windows machines without the VC++
  redistributable installed. Without this flag, the binary would
  need the matching MSVC redist on the target machine.

The combination of `+crt-static` plus the release-profile flags is
what makes `parser.exe` portable: copy it to a clean Windows VM
with no Rust tooling installed and it runs.

### Why the explicit linker + env on this machine

A fully-registered VS install lets cargo do all of this
automatically — that's how GitHub Actions CI works on
`windows-latest` runners, and it's how a fresh install on another
developer's machine would work too. **CI is not affected by these
config entries** because `.cargo/config.toml` is gitignored and the
GH runner's MSVC discovery via `vswhere` works on its own.

The explicit paths exist to work around a one-off issue with this
particular install (the VS installer state didn't finalize, possibly
because the elevation requirement wasn't met during the original
winget run). They're harmless on properly-registered machines: cargo
will still use the explicit linker even when discovery would have
worked.

When VS Build Tools versions change (the `14.44.35207` directory or
the Windows SDK `10.0.26100.0` directory get a new version), update
the paths in this file accordingly. The fresh-machine reproduction
steps in the next section show how a properly-registered machine
gets a minimal version of this file.

## Reproducing the setup on a fresh Windows machine

~5 GB of downloads (most of it Build Tools + SDK), ~30-60 minutes
including the Build Tools install. Run as a user account that's in
the Administrators group; the Build Tools installer needs elevation.

### 1. Install Rust

```powershell
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile $env:TEMP\rustup-init.exe
& $env:TEMP\rustup-init.exe -y --default-toolchain stable --profile default
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

The default `stable` toolchain on Windows is already
`x86_64-pc-windows-msvc`, which is what we want. No further
`rustup default` is needed.

### 2. Install Visual Studio Build Tools 2022 (VCTools workload)

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" `
  --accept-package-agreements --accept-source-agreements
```

`--includeRecommended` adds the Windows 11 SDK and the static MSVC
runtime libraries that `+crt-static` needs.

The bootstrap returns quickly but the actual install runs async via
two `setup.exe` worker processes — wait until both exit before
moving on:

```powershell
Get-Process -Name setup -ErrorAction SilentlyContinue | Wait-Process
```

Total time: 15-30 minutes depending on bandwidth and how much of
the workload is already cached locally.

### 3. Verify the toolchain is wired up

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" `
  -all -prerelease -requires Microsoft.VisualCpp.Tools.HostX64.TargetX64 -property installationPath
Test-Path "C:\Program Files (x86)\Windows Kits\10\Lib"
```

Both should return non-empty. If not, the install didn't finish —
open the Visual Studio Installer GUI at
`C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe`
and modify the install to add the Desktop development with C++
workload (or the "VCTools" id and the Windows SDK component).

### 4. Create `.cargo/config.toml`

If `vswhere` from step 3 returned a non-empty path, the install is
properly registered and cargo's auto-discovery will work. The
minimal config is:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

If `vswhere` returned empty (the install registered partially —
unusual but possible if the installer didn't run elevated), use
the longer form documented in the [`.cargo/config.toml` section
above](#cargoconfigtoml) which pins the linker path and provides
INCLUDE / LIB explicitly.

The file goes at `D:\parser-cli\.cargo\config.toml` (gitignored).

### 5. Build

```powershell
cd D:\parser-cli
cargo build --release
```

This works from a plain PowerShell session — no `vcvars64.bat`
sourcing needed, no "x64 Native Tools Command Prompt" required.
With the minimal `.cargo/config.toml` (auto-discovery path), cargo
finds `link.exe` and the SDK via `vswhere`. With the longer form,
cargo reads the explicit linker and `INCLUDE` / `LIB` directly from
the config file. Either way, `cargo build` is self-sufficient.

The `Installer` directory must be on PATH before vcvars is sourced
because vcvars depends on `vswhere.exe` to find the install location.

Output: `target/release/parser.exe`.

## Useful commands

| Command | What it does |
|---|---|
| `cargo build` | Debug build to `target/debug/parser.exe`. ~30s clean, <2s incremental. |
| `cargo build --release` | Optimized build to `target/release/parser.exe`. ~45s clean (LTO + single codegen-unit are slow). |
| `cargo run -- <args>` | Rebuilds debug then runs with `<args>`. Use `--` to pass args through. |
| `cargo test --bin parser config::tests` | Runs the two unit tests. See [testing.md](testing.md). |
| `cargo check` | Type-checks without producing a binary. Fastest feedback loop. |
| `cargo clean` | Wipes `target/`. Safe; rebuilds on next invocation. |
| `cargo tree` | Prints the resolved dependency tree (130 crates). |

## Disk usage

After a release build:

| Location | Size today |
|---|---|
| `D:/parser-cli/target/` | ~2 GB (build cache, gitignored — currently has both gnullvm and MSVC artifacts; running `cargo clean` and rebuilding cuts to ~1.2 GB MSVC-only) |
| `D:/parser-cli/Cargo.lock` | <10 KB |
| `D:/parser-cli/.cargo/config.toml` | <1 KB |
| `~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/` | ~1.2 GB |
| `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\` | ~3.4 GB |
| `C:\Program Files (x86)\Windows Kits\10\` | ~1.7 GB |
| `~/.cargo/registry/` | ~280 MB (shared across all Rust projects on this machine) |
| `target/release/parser.exe` | **0.99 MB** |

## What this file deliberately doesn't cover

- **Linux / macOS builds.** Untested. The toolchain section assumes
  Windows. Cross-platform release builds are roadmap step 10.
- **CI configuration.** No CI is set up. Adding GitHub Actions or
  similar is a separate concern.
- **Code-signing the binary.** Not needed for development; will be
  needed before public distribution.

## Cross-references

- [main.md](main.md) — how the async runtime gets wired to
  `tokio = { features = ["macros", "rt"] }`.
- [providers.md](providers.md) — why `async-trait` and `futures` are
  in the dep list.
- [config.md](config.md) — how `serde`, `toml`, `dirs`, `url` get used.
- [testing.md](testing.md) — how `tempfile` gets used.
