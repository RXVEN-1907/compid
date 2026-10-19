# Contributing to compid

Thanks for helping. compid is small on purpose, so the contributions that
matter most are concrete: dataset rows, parser robustness, honest docs, and
tests that pin behavior down.

## Setup

- Rust 1.75 or newer (rustup is the easy way).
- Python 3.11 or newer if you are touching the corpus builder or trainer.
- `just` or `make` is optional - both are thin wrappers over the same
  commands.

```console
$ git clone https://github.com/RXVEN-1907/compid
$ cd compid
$ cargo test
```

For dataset and training work, create a virtual environment and install the
training dependencies (see `training/README.md`). Optional toolchains for the
seed matrix - zig for clang-family, musl, and aarch64 builds - are documented
in `corpus/README.md`.

## Where things live

| Path          | What it is                                        |
|---------------|---------------------------------------------------|
| `src/`        | Rust: extractor, model reader, the six subcommands |
| `corpus/`     | Python: builds the seed matrix on this machine     |
| `training/`   | Python: manifest + features to `model.json`        |
| `dataset/`    | manifest (committed), binaries (local), features   |
| `model/`      | the committed `model.json`                         |
| `docs/`       | formats, architecture, roadmap, CLI reference      |
| `tests/`      | cargo tests and fixtures                           |

## Everyday commands

`make` and `just` accept the same targets; here is the make spelling:

| Command        | Does                                        |
|----------------|---------------------------------------------|
| `make build`   | release build of the CLI                    |
| `make test`    | `cargo test`                                |
| `make fmt`     | apply rustfmt                               |
| `make check`   | rustfmt check + clippy (what CI runs)       |
| `make corpus`  | rebuild the seed corpus                     |
| `make train`   | retrain `model/model.json`                  |
| `make clean`   | `cargo clean`                               |

## Ground rules

- `cargo fmt --all -- --check` and `cargo clippy --all-targets` should be
  clean. Keep `rustfmt.toml` minimal and stable-only.
- Documentation is plain U.S. English, no emoji, and every number is either
  backed by a reproducible run or clearly marked as illustrative. No
  hopeful metrics.
- Behavior changes come with tests in `tests/`.
- User-visible changes get a `CHANGELOG.md` entry under **Unreleased**.

## Changing the feature schema or model format

- Feature-schema changes bump the model's `feature_version` and land together
  with a retrain and a refresh of `dataset/features/` and `model/model.json`.
- `model.json` layout changes bump the format string (`compid-model/v1` ->
  the next one) and must update `docs/model-format.md` and the Rust reader in
  the same change, so reader and writer never drift apart.

## Pull requests

One logical change per PR, imperative subject line ("Add musl seed rows",
not "Added musl seed rows"), and the PR template filled out honestly. CI runs
rustfmt, clippy, tests, a release build, and a Python entry-point check; all
five need to pass.

## Adding dataset rows

Follow `corpus/README.md`; the manifest and feature formats are specified in
`docs/dataset.md`. Two hard rules: one unique `build_id` per row, and no
vendored third-party binaries - everything in the dataset is built from
source by the corpus builder, which keeps licensing clean.

## Reporting bugs

Use the bug report template. Include `compid --version`, the model's
`feature_version`, and a binary that reproduces it if you can share one -
never live malware.

## License

By contributing, you agree that your contributions are dual-licensed under
MIT OR Apache-2.0, as defined in `LICENSE`.
