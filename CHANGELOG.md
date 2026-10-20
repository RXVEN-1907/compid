# Changelog

All notable changes to compid are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- PE and Mach-O function sources, closing the function-level roadmap item:
  `compid functions` now covers all three container formats. A PE
  enumerates its export directory - named, non-forwarded exports landing
  in an executable section, with sizes derived from the next export's RVA
  (the section end for the last one; the table records no sizes, and the
  heuristic is documented as such) - while a plain Windows executable
  without an export table keeps its explicit
  "no export table: PE function extraction needs exported symbols this
  round" reason. A Mach-O enumerates its `LC_SYMTAB`: defined, non-stab
  symbols (`n_type & N_TYPE == N_SECT`) whose 1-based section index names
  an executable `__TEXT`-segment section their value falls inside, raw
  names with the ABI's leading underscore kept, and sizes derived from the
  next defined symbol's value in the section (nlist entries carry no
  size), deduplicated by `(value, size)` like the ELF path. The corpus
  seeds PE function rows: `corpus/sources/export_demo.c` carries a
  portable export macro and takes a focused slice (x86_64-windows-gnu
  `-shared` DLL rows at O0/O2 plus linux-gnu executables, deliberately
  outside the standard loops so the deterministic variant slices do not
  shift), and `tests/fixtures/zig-cc_O2_x86_64_dll_sample` is a committed
  PE DLL built by `corpus/build_fixtures.py` from the same source. The
  per-function dataset now spans all three formats (342,507 function rows
  from 1,297 builds: 267,626 ELF, 74,873 Mach-O, 8 PE), and the retrained
  `model/functions.json` holds at hold-out top-1 0.9995 `compiler_family`
  / 0.9204 `opt_level` (build-majority 0.9885 / 0.8423).
- Colored terminal output and a `compid report` one-screen summary. A
  small in-tree ANSI engine (`src/term.rs`, no external crates) renders
  the human tables with a restrained palette: lavender block-character
  bars, the winning class in green, probabilities and metadata dimmed,
  field names and section headers bold, and the `[low confidence]` marker
  in amber. `classify`, `explain`, `functions`, and the new `report`
  subcommand take `--color <auto|always|never>` (default `auto`): color
  appears only when stdout is a terminal, `NO_COLOR` (set and non-empty)
  keeps it plain, an explicit `--color always` forces it on for pipes,
  and `--color never` reproduces the previous byte-for-byte output. JSON
  outputs are never colored. `compid report <BIN>` combines the
  classification table, two-line contributor excerpts for the four most
  confident fields, and the model's provenance block with 12-hex-char
  sha256 short forms; `--json` emits one object combining the
  classification with the full provenance. Plain (non-colored) output is
  pinned by tests to stay byte-identical.
- Parsing-path robustness: a deterministic, in-tree mutation fuzzer
  (`tests/robustness_test.rs`, no external crates, no nightly) that runs
  on every `cargo test` and corrupts each committed fixture in
  `tests/fixtures/` 88 ways - 3,256 cases over 37 fixtures spanning bit
  flips, byte and magic-byte substitution, truncation, duplicated and
  appended section-header tables, inflated length fields
  (`sh_size`/`sh_offset`/section sizes at their type maximum), ELF
  `e_shnum`/`e_shoff`/`e_phnum`/`e_type` header corruption, and
  endianness flips - then pushes every mutant through the full
  user-facing surface (`extract`, `functions` with the stripped-binary
  prologue heuristic, and the classify path) under `catch_unwind` with a
  30-second per-case hang guard. The sweep hardens offset arithmetic on
  untrusted headers to saturating math (the first run found overflow
  panics in the ELF symbol-to-file-offset computation in
  `src/elfinfo.rs` and the prologue scan's alignment/address arithmetic
  in `src/functions.rs`; one further panic lives inside goblin 0.10.7's
  Mach-O `LC_MAIN` handling and is scoped to its exact case in a
  documented allowlist), asserts the `functions`-empty == `reason`-set
  and `meta:`-key invariants on every mutant, and pins the fixed sites
  with named regression cases. Mutations are reproducible: a pinned seed,
  per-case seeds derived from the fixture file name, a single-case
  reproducer (`COMPID_FUZZ_FIXTURE`/`COMPID_FUZZ_CASE`), and corpus
  extension by dropping any ELF/PE/Mach-O into `tests/fixtures/`.
  Coverage-guided (cargo-fuzz) continuous fuzzing remains future work.
- Stripped-binary function boundaries for x86-64 ELF: `compid functions`
  no longer gives up on a stripped binary - a conservative prologue scan
  over the executable sections (frame-pointer prologues and the crt-stub
  paired-`lea` shape at 16-byte alignment, CET `endbr64` sites with
  PLT-stub and followup gates, decode-checked starts merged by straight-
  line overlap, 16-byte minimum) proposes the boundaries with synthesized
  `func_<hex addr>` names, marked `source: heuristic` in `--json` and by a
  header note in the table. Measured, not promised: `compid functions
  <BIN> --measure` scores the heuristic against symbol-table ground truth
  at a ±8-byte tolerance, and the `scripts/measure_boundaries.py` sweep
  over 40 linux-gnu x86_64 manifest rows spanning O0-Os lands at pooled
  precision 0.9806 / recall 0.8588 (F1 0.9157). `functions-batch` now
  emits stripped rows alongside symbol-table ones, and the mixed-demo
  acceptance script runs its gcc/zig majority assertion on a `strip`ped
  copy too (attribution by address against the unstripped run). Non-x86-64
  ELFs, PEs, and Mach-Os keep their explicit reasons; PE/Mach-O symbol
  sources remain future work.
- Call-target analysis for stripped-binary function boundaries: alongside
  the prologue-pattern scan, each executable section of a stripped x86-64
  ELF is now disassembled linearly with the same length-aware decoder, and
  every decoded direct `call rel32` (opcode E8) contributes its target
  (`instruction address + length + signed displacement`) as a candidate
  function start - catching functions that lack a frame-setup prologue
  (cold clones, shrink-wrapped `-Os` entries). A target qualifies when it
  falls inside the scanned section (cross-section branches into `.plt` are
  thereby excluded), decodes cleanly for at least 16 bytes, does not open
  with a PLT-stub shape, and is not already a prologue candidate; the
  union runs through the unchanged decode-aware overlap merge and
  call-target-only starts get the same synthesized `func_<hex addr>`
  names. Direct `jmp rel32` (E9) targets were measured and kept out: they
  may be tail-thunks, but most are loop back-edges and goto joins that
  split correct functions (with E9 the sweep drops to precision 0.4334 /
  recall 0.9215). Measured on the same 40-row O0-Os sweep, the call-target
  scan moves the stripped-boundary heuristic from pooled precision 0.9806
  / recall 0.8588 (F1 0.9157) to precision 0.9819 / recall 0.9174
  (F1 0.9486): 159 more ground-truth starts for 13 more unmatched starts.
  The stripped half of `scripts/mixed_demo.sh` holds with the finer
  boundaries (16 heuristic functions vs 11 before, both majorities
  unchanged).
- Hardened-build axis and hardening detection: the corpus gains 30 builds
  compiled with `-fstack-protector-all`, `-D_FORTIFY_SOURCE=2`, and
  `-fcf-protection=full` (new `hardening` manifest column), and the
  class-weighted retrain learns a tenth field - `hardening` classifies at
  0.9281 top-1 against a 0.978 majority baseline. Both trainers now fit
  class-weighted by default (`--no-balanced` to disable) so skewed axes
  survive their majority priors.
- Abstention signal in `compid classify`: fields whose top prediction
  falls below the confidence threshold (default 0.6, `--confidence-threshold`
  to tune, 0 to disable) print a `[low confidence]` marker in the table and
  carry `"low_confidence"` in the JSON output - a visible "the model is not
  sure here" instead of a bare confident number.
- Model provenance and `compid model-info`: trained models now carry a
  provenance block (manifest row count and sha256, features row count and
  sha256), `compid model-info` prints the fields, classes, and provenance
  of any model file, and classify/function headers state how many builds
  stand behind the model.
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
