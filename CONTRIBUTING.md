# Contributing to TetherCraft

Read [AGENTS.md](AGENTS.md) first: it carries the rules this project is held to. A pull request
that breaks one of them is rejected regardless of quality.

## Getting set up

```powershell
git clone https://github.com/sandraschi/tethercraft
cd tethercraft
cargo build --workspace
cargo test --workspace
```

Rust 1.82 or newer. No Python, Node, database server or service to install.

## Before you open a pull request

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

All three must pass. The workspace denies `unwrap`, `expect`, `panic`, `todo` and
`unimplemented` in production code: test modules opt out with an explicit `#[allow(...)]`
attribute, nothing else does.

## What a good change looks like

- One concern per commit, 10 files at most, message in the imperative ("add reconnect backoff").
- Unbuilt work returns `not_implemented` with a reason. Do not merge a stub that looks finished.
- A bug fix comes with a test that fails before it and passes after.

## Licence

Contributions are dual licensed MIT OR Apache-2.0, matching the project. By opening a pull
request you agree to that.
