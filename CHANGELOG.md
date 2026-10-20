# Changelog

All notable changes to compid are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Function-level provenance for unstripped ELF binaries: `compid functions
  <BIN>` enumerates functions from the ELF symbol table (SHT_SYMTAB,
  STT_FUNC with a size, deduplicated by address and size, raw names;
  relocatable `.o` files included), scores each function's scoped feature
  vector (`ng:`/`mn:`/`rg:`/`cn:` over the function bytes plus a `fn:`
  size bucket and `meta:windows`; no `sec:`/`ent:`/`cmt:`) and prints a
  per-function table with `compiler_family` and `opt_level` predictions
  and a predicted-family tally from `model/functions.json`. `--features
  FILE` writes the per-function training JSONL, and `--json` emits the
  machine-readable form. `compid functions-batch <MANIFEST> --features`
  runs the same extraction over a manifest with the manifest's ground-truth
  labels. Stripped ELFs, PEs, and Mach-Os report an explicit reason instead
  of guessing boundaries; stripped-binary heuristics and PE/Mach-O symbol
  sources remain future work. `training/train_functions.py` trains the
  per-function model with the hold-out split by build (all functions of a
  binary stay on one side), exports the same `compid-model/v1` format,
  and regenerates `docs/FUNCTIONS-EVALUATION.md`. `scripts/mixed_demo.sh`
  is the mechanical acceptance test: a gcc-compiled and a zig-cc-compiled
  corpus source linked into one binary, with each half's functions
  majority-predicting their own toolchain.
- Mach-O (macOS) support in extraction: `compid extract`, `classify`,
  `explain`, and `compare` detect the Mach-O container from its magic
  bytes and read 64-bit images (x86-64 and arm64; 32-bit is rejected with
  a clear error). Mach-O binaries report `os=macos`, the cputype-spelled
  architecture (`x86_64`, `aarch64`), and `pie=pie`; section geometry and
  entropy key on the `__SEGMENT,__section` spelling, dylib load-command
  names feed the `cmt:` namespace as lowercase basenames, and the clean
  disassembly-window count files under the new `meta:macos` feature.
  The build matrix now seeds `x86_64-macos` and `aarch64-macos` rows and
  the retrained model carries a three-way `os` field (linux/windows/macos).
- Windows PE (x86-64) support in extraction and classification: `compid
  extract`, `classify`, `explain`, `compare`, and `batch` detect the
  container from the file's magic bytes. PE binaries report `os=windows`
  and the COFF-machine architecture (`x86_64`), their import-table DLL
  names feed the `cmt:` feature namespace, and the windows rows the corpus
  already records (previously skipped at extraction time) now produce
  features and join training.
- `compid completions <shell>` - shell completion scripts for bash, zsh,
  fish, and PowerShell, printed to stdout for the caller's shell; install
  locations are documented in `docs/cli.md`.
- Ghidra script (`ghidra_scripts/compid_classify.py`) that classifies the
  program under analysis with `compid classify`, appends the per-field
  predictions to a listing plate comment, and prints the full table to the
  script console.
- riscv64 and arm32 matrix axes - the corpus builder, manifest, and model
  now cover riscv64 and arm32 alongside x86-64 and aarch64.

### Security

- Hardened the corpus builder's toolchain lookup and warmup scratch
  directory: toolchain binaries are trust-checked for ownership and
  writability (resolved through symlinks) immediately before every
  execution, and the serial cache warmup compiles in a private per-run
  directory instead of a predictable path under `/tmp`.

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
