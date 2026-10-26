# Contributing to MapleLens

Thanks for helping! Bug reports, ideas and pull requests are welcome.

## Workflow

1. Open an issue first for anything bigger than a small fix, so we agree on the direction.
2. Fork the repo and create a branch from `main`.
3. Install [Rust](https://rustup.rs) (1.88+) and check, on Windows:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets
   cargo test --workspace
   cargo deny check        # if you changed dependencies (cargo install cargo-deny)
   ```
   Changed `crates/core` or `crates/web`? Rebuild the web app's engine with `./scripts/build-web.ps1` and commit
   `docs/app/pkg` too (GitHub Pages serves it as it is).
4. Open a pull request using the template. CI must pass.

`main` is protected: every change lands through a pull request that the maintainer reviews and merges.
Reviews are on a best-effort basis; please be patient.

## Ground rules

Read [AGENTS.md](AGENTS.md): it documents the architecture, conventions and hard rules. In short:

- Screen pixels only: no injection, hooks, memory reading, opening the game process, or input of any kind.
- Never commit MesoWatch data (`data/*.json` is git-ignored) or Nexon artwork.
- The overlay stays click-through and never steals focus.
- Pure logic gets unit tests; overlay changes need a test or a screenshot over the game.

By contributing you agree your work is released under the [MIT license](LICENSE) and that you follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
