# Testing

Step 1 ships with two unit tests, both in
[src/config/mod.rs](src/config/mod.rs:361) under `#[cfg(test)] mod tests`.
They prove the two runtime invariants of `Config::load()` empirically.

## Running the tests

```bash
cargo test --bin parser config::tests
```

Filtering to `config::tests` is optional — bare `cargo test` runs the
same two right now — but it's the most precise form and will keep
working as more tests get added in other modules.

Expected output:

```
running 2 tests
test config::tests::api_key_field_holds_resolved_value_not_env_var_name ... ok
test config::tests::data_dir_is_fully_resolved_pathbuf_not_tilde_string ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## What each test proves

### `api_key_field_holds_resolved_value_not_env_var_name`

Writes a config to a temp directory pointing at the env var
`PARSER_TEST_KEY_INVARIANT_1`, sets that env var to `"sk-or-v1-FAKE-abc123"`,
calls `Config::load_from(...)`, and asserts:

```rust
assert_eq!(cfg.model.api_key_env, "PARSER_TEST_KEY_INVARIANT_1");
assert_eq!(cfg.model.api_key,     "sk-or-v1-FAKE-abc123");
assert_ne!(cfg.model.api_key,     cfg.model.api_key_env);
```

The third assertion is the load-bearing one: it proves the loader didn't
just copy `api_key_env` into `api_key`.

### `data_dir_is_fully_resolved_pathbuf_not_tilde_string`

Writes a config with `data_dir = "~/.parser"` to a temp directory, calls
`Config::load_from(...)`, and asserts:

```rust
let home = dirs::home_dir().expect("home");
assert_eq!(cfg.paths.data_dir, home.join(".parser"));
assert!(cfg.paths.data_dir.is_absolute());
assert!(!cfg.paths.data_dir.to_string_lossy().contains('~'));
```

The second and third assertions are the load-bearing ones: any unresolved
`~` would fail one of them.

## Test infrastructure

```rust
fn write_config(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("parser.config.toml");
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    p
}
```

`tempfile::tempdir()` creates a fresh temp directory under the OS temp
dir for each test. The `TempDir` value is RAII — when it goes out of
scope at the end of the test, the directory is recursively deleted
automatically. **No manual cleanup is needed**; in audit runs, zero
leftover entries were observed.

## Why the tests use `Config::load_from(&Path)` not `Config::load()`

`Config::load()` reads from `~/.parser/parser.config.toml` — your real
config. Tests must not touch that. The internal loader is split so the
path can be injected:

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

If you want to add a test that involves loading from a custom path,
use `load_from` and a `tempfile::tempdir()` — same pattern as the
existing two tests.

## What the tests don't cover (gaps to fill later)

- **Defaults applied when sections are missing.** No test asserts that
  omitting `[parameters]` produces `max_tokens = 4096`, `temperature = 0.7`,
  `context_limit = None`. Worth adding.
- **`MissingField` errors.** No test asserts that omitting
  `model.endpoint` produces `ConfigError::MissingField("model.endpoint")`.
  Worth adding.
- **`InvalidUrl` errors.** Endpoint validation is exercised by `init`'s
  prompt loop but not unit-tested. Worth adding.
- **`init` wizard.** The interactive prompts read from stdin and aren't
  unit-tested. Could be tested by extracting the I/O behind a small
  trait, but for step 1 it's been smoke-tested manually instead.

None of these block step 2. Add them when you next touch the config
loader.

## Manual smoke test for the CLI

Useful when you want to confirm the binary works end-to-end (not just
that the loader does):

```powershell
# Build release
cargo build --release

# Set the env var Config::load() expects to read
$env:OPENROUTER_API_KEY = "sk-or-v1-anything"   # value doesn't have to be real for step 1

# Run all four CLI shapes
.\target\release\parser.exe                          # → help
.\target\release\parser.exe run "fix the jwt bug"    # → 4-line confirmation
.\target\release\parser.exe "fix the jwt bug"        # → same 4-line confirmation
.\target\release\parser.exe init                     # → wizard (will prompt about overwrite)
```

Every command except `init` should exit with status `0` (or `2` for the
help case). `init` exits `0` after writing or `0` when the user declines
to overwrite.

## Adding a test

The pattern:

```rust
#[test]
fn descriptive_name_of_what_is_being_proven() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_config(tmp.path(), r#"
        [model]
        endpoint = "..."
        ...
    "#);
    // optional: set/unset env vars

    let cfg = Config::load_from(&path).expect("load");

    assert_eq!(cfg.<field>, <expected>);
}
```

Test names should describe the assertion, not the input — read the two
existing names for the style.
