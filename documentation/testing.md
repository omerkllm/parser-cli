# Testing

Two unit tests today, both in `src/config/mod.rs` under
`#[cfg(test)] mod tests`. They prove the two load-time invariants
that the rest of the codebase depends on. There are no tests yet
for `agents/` or `providers/` — both are placeholder modules with
no behavior worth testing.

## Running the tests

```powershell
cargo test --bin parser config::tests
```

`--bin parser` selects the binary target (the project has no `lib`
target). `config::tests` filters to the two-test module; bare
`cargo test` runs the same two today, but the explicit filter keeps
working as more tests get added in other modules.

Expected output:

```
running 2 tests
test config::tests::api_key_field_holds_resolved_value_not_env_var_name ... ok
test config::tests::data_dir_is_fully_resolved_pathbuf_not_tilde_string ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## What the two tests prove

### `api_key_field_holds_resolved_value_not_env_var_name`

[src/config/mod.rs:373](src/config/mod.rs:373).

Writes a config to a temp directory pointing at the env var
`PARSER_TEST_KEY_INVARIANT_1`, sets that env var to
`"sk-or-v1-FAKE-abc123"`, calls `Config::load_from(...)`, and
asserts:

```rust
assert_eq!(cfg.model.api_key_env, "PARSER_TEST_KEY_INVARIANT_1");
assert_eq!(cfg.model.api_key,     "sk-or-v1-FAKE-abc123");
assert_ne!(cfg.model.api_key,     cfg.model.api_key_env);
```

The third assertion is the **load-bearing** one: it proves the
loader didn't just copy `api_key_env` into `api_key`. If the loader
ever regresses to copying the env-var name instead of resolving it,
this test fails immediately.

This invariant matters because the rest of the codebase (and Step 3's
provider) reads `model.api_key` directly and never re-checks env
vars — if the value were the env-var *name*, the provider would
authenticate with literal `"OPENROUTER_API_KEY"` and fail.

### `data_dir_is_fully_resolved_pathbuf_not_tilde_string`

[src/config/mod.rs:394](src/config/mod.rs:394).

Writes a config with `data_dir = "~/.parser"` to a temp directory,
calls `Config::load_from(...)`, and asserts:

```rust
let home = dirs::home_dir().expect("home");
assert_eq!(cfg.paths.data_dir, home.join(".parser"));
assert!(cfg.paths.data_dir.is_absolute(), "must be absolute");
assert!(
    !cfg.paths.data_dir.to_string_lossy().contains('~'),
    "tilde must be expanded, got {:?}",
    cfg.paths.data_dir
);
```

The last two assertions are the load-bearing ones. Any unresolved
`~` would fail one of them.

This invariant matters because every later step that touches the
filesystem (the indexer storing AST cache, the compressor writing
the decision log) takes a `&Path` argument. A `~`-prefixed path
would silently fail to resolve on Windows where `~` is not a shell
expansion.

## Test infrastructure

```rust
fn write_config(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("parser.config.toml");
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    p
}
```

[src/config/mod.rs:365](src/config/mod.rs:365). Helper to write a
TOML body to a file inside a temp dir, returning the path.

`tempfile::tempdir()` creates a fresh temp directory under the OS
temp dir for each test. The returned `TempDir` value is **RAII** —
when it goes out of scope at the end of the test, the directory is
recursively deleted automatically. **No manual cleanup is needed.**

In audit runs across dozens of test invocations, zero leftover temp
directories were observed under normal exit. If a test process is
killed mid-run (Ctrl+C, OOM), the temp dir won't be cleaned;
to check:

```powershell
Get-ChildItem $env:TEMP -Filter '.tmp*' -Directory | Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-1) }
```

Anything that shows up is safe to delete.

## Why tests use `Config::load_from(&Path)` not `Config::load()`

`Config::load()` reads from `~/.parser/parser.config.toml` — the
user's real config. Tests must not touch it. The internal loader is
split so the path can be injected:

```rust
impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let path = config_file_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        // actual loading logic
    }
}
```

If you want a test that involves loading from a custom path, use
`load_from` and a `tempfile::tempdir()` — same pattern as the two
existing tests.

## Test env vars

The two tests set process-level env vars:

- `PARSER_TEST_KEY_INVARIANT_1`
- `PARSER_TEST_KEY_INVARIANT_2`

These are set with `std::env::set_var(...)` and only exist in the
test process — they don't persist to your shell or the registry.

Note: `std::env::set_var` is **process-global**, so two tests setting
different values for the same name would race. The current two tests
use distinct names, so they're safe. If you add tests that share a
name, either pick distinct names or run with `--test-threads=1`.

## Coverage gaps (worth filling later)

Things that are **not** unit-tested today:

- **Defaults applied when sections are missing.** No test asserts
  that omitting `[parameters]` produces `max_tokens = 4096`,
  `temperature = 0.7`, `context_limit = None`.
- **`MissingField` errors.** No test asserts that omitting
  `model.endpoint` produces `ConfigError::MissingField("model.endpoint")`.
- **`InvalidUrl` errors.** Endpoint validation is exercised by
  `init`'s prompt loop but not unit-tested.
- **`init` wizard.** The interactive prompts read from stdin.
  Could be tested by extracting the I/O behind a small trait, but
  for now the wizard is smoke-tested manually.
- **`Agent` and `ModelProvider` traits.** No tests yet. Both are
  placeholder modules; first tests land when they have real behavior.

None of these block forward progress. Add them when you next touch
the relevant code path.

## Manual smoke test for the CLI

Useful when confirming the binary works end-to-end (not just that
the loader does):

```powershell
# Build release.
cargo build --release

# Set the env var Config::load() expects to read.
$env:OPENROUTER_API_KEY = "sk-or-v1-anything"   # value doesn't have to be real for Step 2

# Run all four CLI shapes.
.\target\release\parser.exe                          # → help text, exit 2
.\target\release\parser.exe run "fix the jwt bug"    # → 4-line confirmation
.\target\release\parser.exe "fix the jwt bug"        # → same 4-line confirmation
.\target\release\parser.exe init                     # → wizard (prompts about overwrite)
```

Every command except `init` should exit with status `0` (or `2` for
the bare-help path). `init` exits `0` after writing or when the user
declines to overwrite.

Expected output for `parser run "fix the jwt bug"`:

```
Config loaded successfully
Model: <whatever's in your config>
Endpoint: <whatever's in your config>
Coder agent placeholder
```

The fourth line comes from `CoderAgent::run`'s placeholder body —
see [agents.md](agents.md#coderagent--the-placeholder).

## Adding a new test

The pattern, copied from the existing two tests:

```rust
#[test]
fn descriptive_name_of_what_is_being_proven() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_config(tmp.path(), r#"
        [model]
        endpoint = "https://example.com"
        name = "test-model"
        api_key_env = "PARSER_TEST_KEY_<UNIQUE>"
    "#);
    std::env::set_var("PARSER_TEST_KEY_<UNIQUE>", "test-key");

    let cfg = Config::load_from(&path).expect("load");

    assert_eq!(cfg.<field>, <expected>);
}
```

Test names should describe the **assertion**, not the input. Read
the two existing names for the style — they're sentences that read
as the contract under test.

## Cross-references

- [config.md](config.md) — what the two tests are proving about the
  loader.
- [main.md](main.md) — error type widening (the test's `?` calls
  ride the same `Box<dyn Error>` chain).
- [04-toolchain.md](04-toolchain.md) — how to set up the toolchain so tests can
  compile and run.
