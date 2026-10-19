# Roadmap

compid is deliberately small, so the roadmap is a queue, not a promise.
Items are ordered roughly by how much capability they add per unit of work.

## Next

### PE and Mach-O support

The feature schema barely cares about the container. `mn:`, `rg:`, `ng:`,
`cn:`, and `meta:windows` come from the code bytes and work unchanged;
`sec:`, `ent:`, and `cmt:` map onto PE sections and COFF comment records, and
Mach-O sections and notes, with small extraction changes. Labels gain
`windows` and `macos` values for `os`. The zig cross-targets can already
produce PE seed binaries, so the corpus side is ready before the parser is.

### More architectures

riscv64 and arm32 alongside x86-64 and aarch64. The yaxpeax family has
decoders for both, so the work is wiring, not research. Cross-builds via the
existing zig toolchain multiply the matrix the day the decoder lands.
aarch64 is already reachable as a seed target; riscv64 and arm32 are next.

### Function-level classification

Whole-binary provenance first, per-function provenance second. Unstripped
binaries hand over boundaries via the symbol table; stripped ones need
function-boundary heuristics (prologue patterns, call targets, alignment
padding). The output becomes a per-function provenance table, which is what
makes the tool genuinely useful on large real-world binaries assembled from
mixed toolchains and vendored libraries.

### Ghidra and IDA plugins

`model.json` is a static file with a documented format, so plugins can load
it directly - no server, no reimplementation of training, just inference
against the model the repo ships (or a local retrained one). Surface
per-field predictions and the explain view in the disassembler UI.

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
