# Parser documentation

Parser is a CLI coding agent that runs in the terminal. It is model-agnostic
— you bring your own API key and any OpenAI-compatible endpoint.

The project's central thesis: **better context management produces better
output, regardless of which model is used.** Parser does not improve the
model. It improves what the model sees.

## Status

Step **1 of 10** complete. The config system, CLI scaffold, and a
self-contained Windows release binary exist. The provider, indexer,
retriever, assembler, and compressor are not yet built.

## Reading order

| File | Read when |
|---|---|
| [01-overview.md](documentation/01-overview.md) | You want the project thesis, architecture, and the 10-step roadmap. |
| [02-cli.md](documentation/02-cli.md) | You want to know which commands exist and how to call them. |
| [03-config.md](documentation/03-config.md) | You're editing `parser.config.toml`, or want to understand `Config::load()`. |
| [04-toolchain.md](documentation/04-toolchain.md) | You're building from source, reproducing the setup on a new machine, or curious why we use `gnullvm` + llvm-mingw. |
| [05-testing.md](documentation/05-testing.md) | You want to verify the code still behaves correctly, or you want to add tests. |
| [06-cleanup.md](documentation/06-cleanup.md) | You want to free disk space, or fully uninstall everything this project added to your machine. |

## File layout of the repo

```
D:/parser-cli/
├── Cargo.toml                  package + dependencies
├── Cargo.lock                  pinned dep versions (committed)
├── .cargo/config.toml          per-project linker, archiver, rustflags
├── src/
│   ├── main.rs                 CLI entry point + subcommand routing
│   └── config/
│       └── mod.rs              Config struct + loader + `parser init` + tests
├── target/                     build output (gitignored, regenerated)
└── documentation/              you are here
```

External state on this machine that the project depends on:

```
~/.cargo/bin/                                 cargo, rustc, rustup binaries
~/.rustup/toolchains/                         Rust toolchains (gnullvm is active)
~/.toolchains/llvm-mingw-20260421-msvcrt-x86_64/   linker + ar + runtime libs
~/.parser/parser.config.toml                  your real Parser config
```
