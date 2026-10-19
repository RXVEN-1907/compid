# Roadmap

compid is deliberately small, so the roadmap is a queue, not a promise.
Items are ordered roughly by how much capability they add per unit of work.

## Next

### Mach-O support

Windows PE support is done as of this round: the extractor maps `sec:`
and `ent:` onto PE sections, counts import-table DLL names as `cmt:`
features, and disassembles the x86-64 `.text` with the same window scheme,
so the corpus's windows builds produce features and train alongside the
ELF ones. That leaves Mach-O. The mapping is the same shape - Mach-O
sections and note records, with small extraction changes - and `os` gains
a `macos` value once the matrix can seed it.

### More architectures

riscv64 and arm32 are done as of this round: the matrix now covers x86-64,
aarch64, riscv64, and arm32, cross-built through the existing zig
toolchain (riscv64-linux-gnu, riscv64-linux-musl, arm-linux-musleabihf
seed targets). Further architectures follow the same wiring pattern - the
yaxpeax family has decoders for them - so the next ISA is a matter of
demand, not research.

### Function-level classification

Whole-binary provenance first, per-function provenance second. Unstripped
binaries hand over boundaries via the symbol table; stripped ones need
function-boundary heuristics (prologue patterns, call targets, alignment
padding). The output becomes a per-function provenance table, which is what
makes the tool genuinely useful on large real-world binaries assembled from
mixed toolchains and vendored libraries.

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
