# Evaluation

This page reports how the committed model performs on data it was not fitted
on, describes the seed dataset behind those numbers, and gives the exact
commands to reproduce every figure. The dataset is a seed, not a benchmark:
the pipeline is the product, and these numbers will move as the matrix grows.

## The seed dataset

Generated on 2026-10-20 by `corpus/build_corpus.py` on a single Linux build
box (x86_64 host, Ubuntu 24.04 userland). 57 self-contained seed programs
are compiled across every toolchain and flag combination that machine
supports. The matrix started at 849 rows and the same-day expansion added
`riscv64-linux-gnu`, `riscv64-linux-musl`, and `arm-linux-musleabihf` via
zig-cc, taking the manifest to **1155 rows**. All 1155 binaries are in the
feature corpus - the extractor handles both ELF and PE.

| toolchain | what it actually is | targets covered |
|-----------|--------------------|-----------------|
| `gcc13`   | the system gcc, 13.3.0 | `x86_64-linux-gnu` |
| `zig-cc`  | zig's `cc` driver bundling clang 19.1.7 | `x86_64-linux-gnu`, `x86_64-linux-musl` (static), `aarch64-linux-gnu`, `riscv64-linux-gnu`, `riscv64-linux-musl`, `arm-linux-musleabihf`, `x86_64-windows-gnu` |

Label distributions over the 1155-row feature corpus:

| label | values (count) |
|-------|----------------|
| `compiler_family` | clang (763), gcc (392) |
| `compiler_version` | 19.1.7 (763), 13.3.0 (392) |
| `opt_level` | O2 (459), O0 (443), O1 (65), O3 (65), Os (74), Oz (49) |
| `arch` | x86_64 (751), riscv64 (204), aarch64 (98), arm (102) |
| `os` | linux (1057), windows (98) |
| `abi` | gnu (853), musl (302) |
| `pie` | no-pie (740), pie (317), n/a (98) |
| `lto` | off (1107), on (48) |
| `static` | no (731), yes (326), n/a (98) |

Several axes are entangled in this seed: every aarch64, riscv64, and arm
build is also non-PIE (zig measures ET_EXEC for those targets), every musl
build is also static and zig-built, gcc is the only toolchain that produced
O1, O3, and Os, zig is the only one that produced Oz, and Windows is only
represented by zig x86-64 PE builds. Labels come from the build matrix, not
from inspection, so these correlations are properties of the matrix, not
noise in the labels.

## How the model is trained

One multinomial logistic regression per label field
(`LogisticRegression(max_iter=3000, C=4.0)`), fitted on L2-normalized integer
count vectors. The raw corpus in `dataset/features/train.jsonl` holds
1,592,176 distinct features across 1155 binaries; the trainer keeps features
seen in at least 150 binaries (`--min-df 150`), which leaves a 43,896-feature
vocabulary and a 13.8 MB `model/model.json` covering all nine label fields.
Each field is a stratified 80/20 split with seed 42; the hold-out is 231
binaries.

## Results (hold-out, seed 42)

| field | classes | top-1 | top-3 | macro-F1 |
|-------|---------|-------|-------|----------|
| `compiler_family` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `compiler_version` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `opt_level` | 6 | 0.8918 | 0.9784 | 0.7922 |
| `arch` | 4 | 1.0000 | 1.0000 | 1.0000 |
| `os` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `abi` | 2 | 0.9567 | 1.0000 | 0.9418 |
| `pie` | 3 | 1.0000 | 1.0000 | 1.0000 |
| `lto` | 2 | 0.9567 | 1.0000 | 0.4889 |
| `static` | 3 | 1.0000 | 1.0000 | 1.0000 |

Reading the weak spots honestly:

- `opt_level` at 89% top-1 is dominated by O2/O3 confusion (O3 is the
  smallest gcc-compiled class and its code shape is a strict subset of what
  O2 emits on this corpus). Top-3 covers 98%.
- `lto` top-1 equals its majority baseline: with 48 LTO rows in 1155, the
  field needs more LTO-built rows before its macro-F1 means anything.
- `abi`'s misses are musl rows the model calls gnu - the musl label is only
  reachable through zig in this seed, so part of what it learns is "zig
  musl-target triple" rather than musl libc linkage itself.
- `pie` and `static` now carry the PE rows' `n/a` class, which the model
  separates cleanly - that is why their top-1 is 1.0000 with more classes
  than the 849-row evaluation.

## Reproducing

```console
$ python3 corpus/build_corpus.py --jobs 16        # rebuild the matrix (resumable)
$ cargo build --release
$ ./target/release/compid batch dataset/manifest.csv --out dataset/features/train.jsonl
$ .venv/bin/python training/train.py              # writes model/model.json + this page's tables
```

`training/train.py` prints the per-field table at the end of the run; the
split is deterministic (`random_state=42`), so reruns reproduce these
numbers byte for byte.

## Limitations

- One build box, two toolchains, seven targets. A binary from a toolchain
  the seed never saw will get a confident wrong answer.
- Confidence values are calibrated to this seed's class balance; on real
  malware or firmware distributions the priors differ.
- The 43,896-feature vocabulary is dominated by x86-64 rows (751 of 1155);
  cross-architecture features rest on byte n-grams and section layout rather
  than mnemonics for riscv64/arm until architecture-specific decoders are
  wired in.
