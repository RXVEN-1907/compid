# Roadmap

compid is deliberately small, so the roadmap is a queue, not a promise.
Items are ordered roughly by how much capability they add per unit of work.

## Next

### macOS dataset rows

Done as of this round. The extractor parses 64-bit Mach-O images (x86-64
and arm64), maps `sec:` and `ent:` onto the segments' sections
(`__TEXT,__text` and friends), counts dylib load-command names as `cmt:`
features, files the clean-window count under `meta:macos`, and
disassembles the x86-64 `__text` with the same window scheme, skipping the
disassembly features on arm64 the way a non-x86-64 PE does. The corpus
builder seeds `x86_64-macos` and `aarch64-macos` rows (204 of them), and
the retrained model carries a three-way `os` field (linux/windows/macos)
at 1.0000 top-1 on the hold-out.

### More architectures

riscv64 and arm32 are done as of this round: the matrix now covers x86-64,
aarch64, riscv64, and arm32, cross-built through the existing zig
toolchain (riscv64-linux-gnu, riscv64-linux-musl, arm-linux-musleabihf
seed targets). Further architectures follow the same wiring pattern - the
yaxpeax family has decoders for them - so the next ISA is a matter of
demand, not research.

### Function-level classification

Unstripped ELF done as of this round: `compid functions` enumerates
functions from the ELF symbol table (relocatable objects included), scores
each function's scoped feature vector with `model/functions.json`
(`compiler_family` and `opt_level`), and `compid functions-batch --features`
feeds `training/train_functions.py`, whose hold-out splits by build. The
mixed-toolchain proof runs mechanically as `scripts/mixed_demo.sh`. Still
ahead: stripped-binary function-boundary heuristics (prologue patterns,
call targets, alignment padding) and PE/Mach-O symbol sources, after which
the output is a complete per-function provenance table for binaries
assembled from mixed toolchains and vendored libraries.

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

Blend new manifest rows into an existing model without a full retrain
(warm-started optimization), plus model provenance metadata in `model.json`
(manifest row count, manifest sha256) so a user can tell how fresh the model
they are using actually is. Scheduled full retrains stay the source of truth;
online updates are a convenience for people feeding it new builds weekly.

## Later / under consideration

- Confidence calibration and an abstention signal: "none of these classes
  fits well" beats a confident wrong answer.
- Hardened-build flag axes in the matrix (`-fstack-protector-all`, CFI,
  shadow stacks) for realism on modern binaries.
- Continuous fuzzing of the extractor's parsing path, since it eats
  untrusted bytes.
- Prebuilt release binaries for common platforms.

## Non-goals

- Replacing disassemblers or BinDiff-style diffing. compid is a fast first
  pass that the heavier tools can sit on top of.
- Guaranteed accuracy on toolchains the seed never saw. The confidence
  scores exist precisely to be honest about that.
