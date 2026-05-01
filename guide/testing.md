# Testing

Parser has 16 unit tests today. They live alongside the source
code in `#[cfg(test)] mod tests` blocks and run via `cargo test`.

## Running tests

### Run everything

```
cargo test --bin parser
```

This runs all 16 tests across both modules. Expected output ends
with:

```
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Run just the config tests

```
cargo test --bin parser config::tests
```

The 14 tests in `src/config/mod.rs`. Useful when you're working
on the loader or validation rules.

### Run just the agent tests

```
cargo test --bin parser agents::tests
```

The 2 tests in `src/agents/mod.rs`. These are async (they use
`#[tokio::test]`), so they take an extra moment to start up the
runtime.

### Run a single test by name

```
cargo test --bin parser endpoint_trailing_slash_is_stripped_silently
```

The argument after `--bin parser` is a substring filter — any
test name containing that substring runs.

## Test inventory

Each test name is a sentence that describes what's being proven.

### `src/config/mod.rs` — 14 tests

| Test | Proves |
|---|---|
| `api_key_field_holds_resolved_value_not_env_var_name` | The loader resolves the env-var value into `model.api_key` rather than copying the env-var *name* into it. |
| `data_dir_is_fully_resolved_pathbuf_not_tilde_string` | A `~/...` path is expanded to an absolute path before `Config` is returned, with no literal `~` left in the resolved value. |
| `endpoint_trailing_slash_is_stripped_silently` | A trailing `/` on the endpoint URL is silently stripped after URL validation. |
| `model_name_longer_than_max_returns_invalid_field` | A model name longer than 200 characters is rejected with `InvalidField` targeting `model.name`. |
| `whitespace_only_model_name_is_rejected_by_validate` | `validate_model_name` rejects whitespace-only input with `InvalidField` for `model.name`. |
| `api_key_env_containing_whitespace_returns_invalid_field` | An env-var name containing whitespace is rejected before any env lookup happens. |
| `api_key_env_longer_than_max_returns_invalid_field` | An env-var name longer than 200 characters is rejected. |
| `blank_api_key_returns_invalid_api_key` | A resolved API key that is whitespace-only is rejected with `InvalidApiKey`. |
| `api_key_with_newline_returns_invalid_api_key` | A resolved API key containing `\n` is rejected with `InvalidApiKey`. |
| `api_key_with_surrounding_quotes_returns_invalid_api_key` | A resolved API key starting and ending with `"` is rejected with `InvalidApiKey`. |
| `temperature_out_of_range_returns_invalid_field` | `temperature = 3.0` is rejected with `InvalidField` targeting `parameters.temperature`. |
| `max_tokens_out_of_range_returns_invalid_field` | `max_tokens = 0` is rejected with `InvalidField` targeting `parameters.max_tokens`. |
| `context_limit_out_of_range_returns_invalid_field` | `context_limit = 3_000_000` is rejected with `InvalidField` targeting `parameters.context_limit`. |
| `context_limit_below_max_tokens_returns_invalid_field` | `context_limit = 1000` with `max_tokens = 4096` is rejected — context must be strictly greater than the output cap. |

### `src/agents/mod.rs` — 2 tests

| Test | Proves |
|---|---|
| `whitespace_only_task_returns_task_empty` | A task containing only whitespace fails the same as `""`, returning `AgentError::TaskEmpty`. |
| `task_longer_than_max_returns_task_too_long` | A task longer than 32,768 characters after trimming is rejected with `AgentError::TaskTooLong { length, max }` carrying the actual lengths. |

## Adding a new test

The pattern, copied from existing tests:

```rust
#[test]
fn descriptive_name_of_what_is_being_proven() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_config(
        tmp.path(),
        r#"
        [model]
        endpoint = "https://openrouter.ai/api/v1"
        name = "x"
        api_key_env = "PARSER_TEST_KEY_INVARIANT_<UNIQUE_NUMBER>"
        "#,
    );
    std::env::set_var("PARSER_TEST_KEY_INVARIANT_<UNIQUE_NUMBER>", "test-key");

    let result = Config::load_from(&path);

    // Assert the specific outcome you want to prove.
    assert!(matches!(result, Err(ConfigError::InvalidField { .. })));
}
```

Three rules to follow:

1. **Use `Config::load_from(&path)`, not `Config::load()`.** The
   bare `load()` reads the user's real
   `~/.parser/parser.config.toml` — tests must never touch that.
   `load_from` accepts an explicit path, which the test points
   at a `tempfile::tempdir()` it controls.
2. **Use a unique `PARSER_TEST_KEY_INVARIANT_<N>` env var name
   per test that needs one.** Tests run in parallel by default,
   and `std::env::set_var` is process-global. Two tests using
   the same name would race.
3. **Name the test as a sentence describing the assertion.**
   Read the existing names — they're sentences that read as the
   contract under test. `endpoint_trailing_slash_is_stripped_silently`
   is good. `test_endpoint` is not.

For an async agent test, use `#[tokio::test]` instead of `#[test]`
and `await` the agent call. See
`src/agents/mod.rs::tests::whitespace_only_task_returns_task_empty`
for the template.

## Linting

### Run clippy

```
cargo clippy -- -D warnings
```

`-D warnings` promotes warnings to errors so a single warning
fails the run. CI uses this exact command, so running it locally
catches anything that would break on push.

### Check formatting

```
cargo fmt --check
```

Exits non-zero if any file would be reformatted by `cargo fmt`.
CI runs this too.

### Fix formatting

```
cargo fmt
```

Reformats every source file in place. Run this if `cargo fmt
--check` flagged anything.

## What's not tested

A few things are intentionally not covered yet:

- **The `parser init` wizard.** Stdin-driven prompts are awkward
  to fake. Smoke-tested manually instead.
- **Provider implementations.** No real provider exists yet —
  `NoopProvider` is a compile stub. Provider tests land when the
  real OpenAI-compatible provider does.
- **End-to-end runs hitting a real model.** Tests don't make
  network calls. CI would have to set up either a sandbox or a
  mock for that.

These gaps are tracked in
[`documentation/testing.md`](../documentation/testing.md) (the
deeper technical doc).
