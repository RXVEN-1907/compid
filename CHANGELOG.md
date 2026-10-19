# Changelog

All notable changes to compid are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-20

### Added

- `compid extract` - per-binary feature JSON over the `.text` section: byte
  n-grams, mnemonics, registers, immediates, section sizes, per-section
  entropy, clean-disassembly windows, and comment/note tokens.
- `compid classify` - per-field softmax predictions with top-3 classes and
  confidence scores, printed as a human table or emitted with `--json`.
- `compid explain` - the top contributing features behind each predicted
  class, human-readable.
- `compid compare` - cosine similarity between the feature vectors of two
  binaries.
- `compid dataset-info` - counts per label value from the dataset manifest.
- `compid batch` - bulk feature extraction over a manifest, with per-row
  missing-binary reporting.
- `compid-model/v1` - a versioned linear softmax model format shared by the
  Python trainer and the Rust reader, documented in `docs/model-format.md`.
- Seed corpus builder that discovers the toolchains available on the build
  machine (system gcc plus a zig `cc` driver today), compiles the seed
  sources across the flag matrix, and records a full manifest.
- Python trainer that turns `dataset/manifest.csv` plus extracted features
  into `model/model.json`.
- Documentation set: architecture, dataset, model format, roadmap, CLI.
- CI workflow (rustfmt check, clippy, cargo test, release build, Python
  entry-point check) and a manual corpus smoke workflow.
- MIT OR Apache-2.0 dual licensing.
