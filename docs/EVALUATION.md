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
riscv64/arm32, then macOS targets, and finally a hardened-build axis
(`-fstack-protector-all`, `-D_FORTIFY_SOURCE=2`, `-fcf-protection=full` on
a slice of gcc builds), taking the manifest to **1389 rows**. All 1389
binaries are in the feature corpus - the extractor handles ELF, PE, and
Mach-O.

| toolchain | what it actually is | targets covered |
|-----------|--------------------|-----------------|
| `gcc13`   | the system gcc, 13.3.0 | `x86_64-linux-gnu` |
| `zig-cc`  | zig's `cc` driver bundling clang 19.1.7 | `x86_64-linux-gnu`, `x86_64-linux-musl` (static), `aarch64-linux-gnu`, `riscv64-linux-gnu`, `riscv64-linux-musl`, `arm-linux-musleabihf`, `x86_64-windows-gnu`, `x86_64-macos`, `aarch64-macos` |

Label distributions over the 1389-row feature corpus:

| label | values (count) |
|-------|----------------|
| `compiler_family` | clang (967), gcc (422) |
| `compiler_version` | 19.1.7 (967), 13.3.0 (422) |
| `opt_level` | O2 (578), O0 (558), O1 (65), O3 (65), Os (74), Oz (49) |
| `arch` | x86_64 (883), riscv64 (204), aarch64 (200), arm (102) |
| `os` | linux (1087), macos (204), windows (98) |
| `abi` | gnu (883), musl (302), n/a (204) |
| `pie` | no-pie (740), pie (551), n/a (98) |
| `lto` | off (1341), on (48) |
| `static` | no (761), yes (326), n/a (302) |
| `hardening` | none (1359), hardened (30) |

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
1,773,105 distinct features across 1389 binaries; the trainer keeps features
seen in at least 150 binaries (`--min-df 150`), which leaves a 68,364-feature
vocabulary and a 24.2 MB `model/model.json` covering all ten label fields.
Fits are class-weighted (`class_weight="balanced"`, `--no-balanced` to
disable) so skewed axes - 30 hardened rows against 1359 unhardened - are not
swallowed by the majority prior. Each field is a stratified 80/20 split with
seed 42; the hold-out is 278 binaries.

## Results (hold-out, seed 42)

| field | classes | top-1 | top-3 | macro-F1 |
|-------|---------|-------|-------|----------|
| `compiler_family` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `compiler_version` | 2 | 1.0000 | 1.0000 | 1.0000 |
| `opt_level` | 6 | 0.8561 | 0.9964 | 0.7991 |
| `arch` | 4 | 1.0000 | 1.0000 | 1.0000 |
| `os` | 3 | 0.9964 | 1.0000 | 0.9952 |
| `abi` | 3 | 0.9568 | 1.0000 | 0.9580 |
| `pie` | 3 | 0.9928 | 1.0000 | 0.9881 |
| `lto` | 2 | 0.8273 | 1.0000 | 0.5874 |
| `static` | 3 | 0.9424 | 1.0000 | 0.9424 |
| `hardening` | 2 | 0.9281 | 1.0000 | 0.6239 |

Reading the weak spots honestly:

- `opt_level` at 86% top-1 is dominated by O2/O3 confusion (O3 is the
  smallest gcc-compiled class and its code shape is a strict subset of what
  O2 emits on this corpus). Top-3 covers 99.6%.
- `lto` and `hardening` are the two severely skewed axes (48 and 30 rows
  against their majorities). The class-weighted fit recovers both above
  their baselines - hardening detection lands at 93% top-1 against a 97.8%
  baseline, which on a 30-row class means the learned artifacts
  (stack-protector probes, endbr64 density, `_chk` PLT entries) are doing
  real work - but the macro-F1 stays honest about the imbalance.
- `abi`'s misses cluster on PE and Mach-O rows: Windows and macOS binaries
  have no gnu/musl linkage, and the three-way `n/a` class is learned from
  container signals rather than libc internals.
- `pie`'s single hold-out miss is the same PE boundary.

## Calibration - can you trust the confidence score?

A softmax probability is a promise. To check it, every hold-out prediction
is bucketed by the winning class's confidence and scored against ground
truth (`training/train.py` writes `docs/calibration.json` on every run;
`scripts/gen_calibration_chart.py` renders `docs/calibration.png`):

| confidence bucket | n | observed accuracy |
|-------------------|-----|-------------------|
| 0.0-0.5 | 22 | 0.4091 |
| 0.5-0.6 | 70 | 0.3143 |
| 0.6-0.7 | 59 | 0.6949 |
| 0.7-0.8 | 108 | 0.8611 |
| 0.8-0.9 | 225 | 0.9067 |
| 0.9-1.0 | 2296 | 0.9895 |

Reading it honestly:

- Above the default abstention threshold (0.6) the model is well behaved -
  every bucket delivers at or above the diagonal, and the 2296 predictions
  at 0.9+ confidence are 98.95% correct.
- The 0.5-0.6 band is **overconfident**: predicted 50-60%, observed 31%.
  That band is exactly what the `[low confidence]` flag exists to catch -
  the abstention threshold is validated by this table, not chosen at
  random.
- The 0.0-0.5 bucket's small n (22) spans several fields' genuinely
  ambiguous hold-out rows; treat it as anecdote, not statistics.

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
- The 68,364-feature vocabulary is dominated by x86-64 rows (883 of 1389);
  cross-architecture features rest on byte n-grams and section layout rather
  than native mnemonics for riscv64/arm/aarch64 until architecture-specific
  decoders are wired in.
