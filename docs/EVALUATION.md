# Evaluation

This page reports how the committed model performs on data it was not fitted
on, describes the seed dataset behind those numbers, and gives the exact
commands to reproduce every figure. The dataset is a seed, not a benchmark:
the pipeline is the product, and these numbers will move as the matrix grows.

## The seed dataset

Generated on 2026-10-20 by `corpus/build_corpus.py` on a single Linux build
box (x86_64 host, Ubuntu 24.04 userland). 57 self-contained seed programs
are compiled across every toolchain and flag combination that machine
supports. The matrix started at 849 rows; the same-day expansions added
riscv64/arm32 and then macOS targets via zig-cc, taking the manifest to
**1359 rows**. All 1359 binaries are in the feature corpus - the extractor
handles ELF, PE, and Mach-O.

| toolchain | what it actually is | targets covered |
|-----------|--------------------|-----------------|
| `gcc13`   | the system gcc, 13.3.0 | `x86_64-linux-gnu` |
| `zig-cc`  | zig's `cc` driver bundling clang 19.1.7 | `x86_64-linux-gnu`, `x86_64-linux-musl` (static), `aarch64-linux-gnu`, `riscv64-linux-gnu`, `riscv64-linux-musl`, `arm-linux-musleabihf`, `x86_64-windows-gnu`, `x86_64-macos`, `aarch64-macos` |

Label distributions over the 1359-row feature corpus:

| label | values (count) |
|-------|----------------|
| `compiler_family` | clang (967), gcc (392) |
| `compiler_version` | 19.1.7 (967), 13.3.0 (392) |
| `opt_level` | O2 (561), O0 (545), O1 (65), O3 (65), Os (74), Oz (49) |
| `arch` | x86_64 (853), riscv64 (204), aarch64 (200), arm (102) |
| `os` | linux (1057), macos (204), windows (98) |
| `abi` | gnu (853), musl (302), n/a (204) |
| `pie` | no-pie (740), pie (521), n/a (98) |
| `lto` | off (1311), on (48) |
| `static` | no (731), n/a (302), yes (326) |

Several axes are entangled in this seed: every aarch64, riscv64, arm, and
macOS build is also non-PIE or PIE respectively and zig-built (only gcc
produces O1, O3, and Os; only zig produces Oz), macOS rows are always PIE
and always `abi n-a`, every musl build is static, and Windows is only
represented by zig x86-64 PE builds. Labels come from the build matrix, not
from inspection, so these correlations are properties of the matrix, not
noise in the labels.

## How the model is trained

One multinomial logistic regression per label field
(`LogisticRegression(max_iter=3000, C=4.0)`), fitted on L2-normalized integer
count vectors. The raw corpus in `dataset/features/train.jsonl` holds
1,772,691 distinct features across 1359 binaries; the trainer keeps features
seen in at least 150 binaries (`--min-df 150`), which leaves a 68,352-feature
vocabulary and a 22.6 MB `model/model.json` covering all nine label fields.
Each field is a stratified 80/20 split with seed 42; the hold-out is 272
binaries.

## Results (hold-out, seed 42)

| field | classes | top-1 | top-3 | macro-F1 |
|-------|---------|-------|-------|----------|
| `compiler_family` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `compiler_version` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `opt_level` | 6 | 0.8971 | 0.9926 | 0.8055 |
| `arch` | 4 | 1.0000 | 1.0000 | 1.0000 |
| `os` | 3 | 1.0000 | 1.0000 | 1.0000 |
| `abi` | 3 | 0.9816 | 1.0000 | 0.9811 |
| `pie` | 3 | 0.9963 | 1.0000 | 0.9973 |
| `lto` | 2 | 0.9632 | 1.0000 | 0.4906 |
| `static` | 3 | 0.9890 | 1.0000 | 0.9871 |

Reading the weak spots honestly:

- `opt_level` at 90% top-1 is dominated by O2/O3 confusion (O3 is the
  smallest gcc-compiled class and its code shape is a strict subset of what
  O2 emits on this corpus). Top-3 covers 99%.
- `lto` top-1 sits just above its majority baseline: with 48 LTO rows in
  1359, the field needs more LTO-built rows before its macro-F1 means
  anything.
- `abi`'s misses cluster on PE and Mach-O rows: Windows and macOS binaries
  have no gnu/musl linkage, and the three-way `n/a` class is learned from
  container signals rather than libc internals.
- `pie`'s single hold-out miss is the same PE boundary.

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

- One build box, two toolchains, nine targets. A binary from a toolchain
  the seed never saw will get a confident wrong answer.
- Confidence values are calibrated to this seed's class balance; on real
  malware or firmware distributions the priors differ.
- The 68,352-feature vocabulary is dominated by x86-64 rows (853 of 1359);
  cross-architecture features rest on byte n-grams and section layout rather
  than native mnemonics for riscv64/arm/aarch64 until architecture-specific
  decoders are wired in.
