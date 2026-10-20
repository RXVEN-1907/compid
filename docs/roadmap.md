# Roadmap

compid is deliberately small, so the roadmap is a queue, not a promise.
Items are ordered roughly by how much capability they add per unit of work.

## Next

### Function-level classification

Done, all three container formats. `compid functions` enumerates functions
from the ELF symbol table (relocatable objects included), scores each
function's scoped feature vector with `model/functions.json`
(`compiler_family` and `opt_level`), and `compid functions-batch --features`
feeds `training/train_functions.py`, whose hold-out splits by build. The
mixed-toolchain proof runs mechanically as `scripts/mixed_demo.sh`.

Stripped x86-64 ELF is covered too, measured rather than promised: a
conservative prologue scan unioned with direct call-target analysis
proposes the boundaries (`source: heuristic`, synthesized `func_<hex addr>`
names), `compid functions <BIN> --measure` scores it against symbol ground
truth at a ±8-byte tolerance, and the 40-binary O0-Os sweep lands at pooled
precision 0.9819 / recall 0.9174 (F1 0.9486), up from 0.9806 / 0.8588
(F1 0.9157) for the prologue scan alone - numbers and residual misses
documented in `docs/FUNCTIONS-EVALUATION.md`. `scripts/mixed_demo.sh`
extends the majority assertion to a `strip`ped copy of the mixed binary
and holds.

PE and Mach-O symbol sources are done as of this round, which closes the
function-level item: a PE enumerates its export directory (named exports
in an executable section; sizes derived from the next export's RVA, the
section end for the last one - documented, since the export table carries
no sizes), the matrix seeds matching windows-DLL rows
(`export_demo-zig-cc-x86_64-windows-gnu-dll-*`), and a Mach-O enumerates
its `LC_SYMTAB` (defined non-stab symbols in executable `__TEXT` sections,
raw names with the ABI's leading underscore, next-symbol-derived sizes).
The per-function dataset now spans all three formats (267,626 ELF, 74,873
Mach-O, and 8 PE function rows), and the hold-out numbers moved up with
the added coverage. What remains open is heuristic rather than symbolic:
boundaries for a plain PE without an export table and for a stripped
Mach-O (prologue-style scans over `.text` / `__text`), and per-function
`static`-side details the export table cannot see (non-exported DLL
functions, Mach-O sizes that only DWARF would pin down).

### Ghidra and IDA plugins

The Ghidra side starts this round: `ghidra_scripts/compid_classify.py`
classifies the program under analysis and appends the per-field predictions
to a listing plate comment. Still ahead is deeper Ghidra integration -
per-function predictions and the explain view surfaced in the disassembler
UI - and the IDA equivalent. `model.json` is a static file with a
documented format, so plugins can load it directly - no server, no
reimplementation of training, just inference against the model the repo
ships (or a local retrained one).

### Online model updates

Half done as of this round: `model.json` now carries a provenance block
(manifest row count and sha256, features row count and sha256), `compid
model-info` renders it, and classify/function headers say how many builds
stand behind the model. Still ahead: warm-started blending of new manifest
rows into an existing model without a full retrain. Scheduled full retrains
stay the source of truth; online updates would be a convenience for people
feeding it new builds weekly.

## Done

### Parsing-path robustness (deterministic mutation fuzzing)

The extractor's parsing path is fuzzed on every `cargo test` now, in-tree
and dependency-free: `tests/robustness_test.rs` is a seeded xorshift64
mutation fuzzer that corrupts every committed fixture in `tests/fixtures/`
88 ways each (3,256 deterministic cases across bit flips, byte and
magic-byte substitution, truncation, duplicated/appended section-header
tables, inflated length fields, header-field corruption, and endianness
flips) and pushes every mutant through the full user-facing surface -
`extract_binary`, `extract_binary_functions` (stripped-binary prologue
heuristic included), and the classify path - under `catch_unwind` with a
per-case timeout. The assertions: no panic anywhere (the one panic that
lives inside goblin 0.10.7's Mach-O `LC_MAIN` arithmetic is scoped to its
exact case and documented in the allowlist), no hang, and the documented
invariants hold. The first run found four overflow sites in compid's own
code - the symbol-to-file-offset additions in `src/elfinfo.rs` and the
prologue-alignment and synthesized-address additions in `src/functions.rs`
- all fixed with saturating arithmetic that cannot change results on valid
binaries (`scripts/mixed_demo.sh` and the `--measure` sweep confirm the
boundary numbers), and pinned by regression cases in the suite. The same
seed always produces the same mutations, single cases are reproducible via
environment variables, and extending the corpus is a file drop into
`tests/fixtures/`. Continuous coverage-guided fuzzing (cargo-fuzz) remains
open.

### Hardened-build axis and hardening detection

The matrix gained 30 hardened builds (`-fstack-protector-all`,
`-D_FORTIFY_SOURCE=2`, `-fcf-protection=full` on a gcc slice, recorded in a
`hardening` manifest column), and the retrained model learns a tenth field:
`hardening` at 0.9281 top-1 against a 0.978 majority baseline. Both trainers
fit class-weighted by default now.

### macOS dataset rows

The extractor parses 64-bit Mach-O images (x86-64 and arm64), maps `sec:`
and `ent:` onto the segments' sections (`__TEXT,__text` and friends),
counts dylib load-command names as `cmt:` features, files the clean-window
count under `meta:macos`, and disassembles the x86-64 `__text` with the
same window scheme. The matrix seeds `x86_64-macos` and `aarch64-macos`
rows (204), and the model's `os` field is three-way (linux/windows/macos)
at 1.0000 top-1 on the hold-out.

### More architectures

The matrix covers x86-64, aarch64, riscv64, and arm32, cross-built through
the existing zig toolchain. Further architectures follow the same wiring
pattern - the yaxpeax family has decoders for them - so the next ISA is a
matter of demand, not research.

### Model provenance

`model.json` carries a provenance block (manifest row count and sha256,
features row count and sha256); `compid model-info` renders it, and report
headers state how many builds stand behind a model.

## Later / under consideration

- Confidence calibration: full per-field calibration curves remain open.
  The abstention half is done as of this round - `classify` flags fields
  whose top prediction falls below the confidence threshold (default 0.6,
  `--confidence-threshold` to tune, 0 to disable), in both the table and
  the JSON output.
- Hardened-build flag axes in the matrix (`-fstack-protector-all`, CFI,
  shadow stacks) for realism on modern binaries.
- Continuous fuzzing of the extractor's parsing path: the deterministic
  in-tree mutation fuzzer (see Done) covers every committed fixture on
  every `cargo test`; continuous coverage with a coverage-guided fuzzer
  (cargo-fuzz on a corpus that grows beyond the fixtures) remains open.
- Prebuilt release binaries for common platforms.

## Non-goals

- Replacing disassemblers or BinDiff-style diffing. compid is a fast first
  pass that the heavier tools can sit on top of.
- Guaranteed accuracy on toolchains the seed never saw. The confidence
  scores exist precisely to be honest about that.
