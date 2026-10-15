# Contributing to Lifeline

Thanks for taking the time. Bug reports, fixes, data-source improvements and
better models are all welcome. For anything larger than a small fix, open an
issue first so we can agree on the approach before you spend time on it.

## Setting up

You need [Rust](https://rustup.rs) 1.91 or newer, Node 20.19 or 22.12 or
newer, and the [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/)
for your OS.

```bash
git clone https://github.com/sekaikx/lifeline.git
cd lifeline
npm install
npm run tauri dev
```

The engine and the `lifeline` command build without Node or Tauri:

```bash
cargo build
cargo run --release -p lifeline-cli -- plan
```

## Checks

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the
following on every push and pull request. Run them locally before opening a
pull request:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -q --release -p lifeline-cli -- plan
cargo run -q --release -p lifeline-cli -- airdrop montecarlo -n 500

npm ci
npm run build                               # TypeScript check and Vite build
cargo clippy -p lifeline-app -- -D warnings # the Tauri crate
```

`cargo test` covers the engine crates and the CLI. The desktop crate in
`src-tauri` is outside the default workspace members, so it is only built
when you name it (`-p lifeline-app`) or run `npm run tauri`.

Tests must not need a network. Recorded responses for Overpass, MET Norway
and Open-Meteo live in
[`crates/geodata/tests/fixtures`](crates/geodata/tests/fixtures); add a
fixture there when you touch a parser.

## Style

- Format Rust with `rustfmt` defaults and keep Clippy clean. Validation
  uses `!(x > 0.0)` on purpose, because it also rejects NaN; the workspace
  allows that lint.
- The engine crates (`crates/`) have no UI dependencies. Anything the app
  shows should be reachable from the `lifeline` command as well.
- Physical quantities are SI internally (metres, seconds, kilograms). Name
  or document the unit when it is not obvious.
- When a change alters a number quoted in the README, `docs/physics.md` or
  `ml/README.md`, update the text in the same pull request.
- Keep commits focused, with a message that says what changed and why.

## Flood model

The training code is in [`ml/`](ml/README.md). Do not commit Sen1Floods11
data or trained weights: the dataset has no explicit licence. If you
improve the model, include the evaluation tables from `ml/evaluate.py` and
`ml/field_check.py` in the pull request.

## Security

Please do not report vulnerabilities in public issues. See
[SECURITY.md](SECURITY.md).

By contributing you agree that your work is released under the
[MIT licence](LICENSE).
