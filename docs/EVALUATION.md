# Evaluation

This page reports how the committed model performs on data it was not fitted
on, describes the seed dataset behind those numbers, and gives the exact
commands to reproduce every figure. The dataset is a seed, not a benchmark:
the pipeline is the product, and these numbers will move as the matrix grows.

## The seed dataset

Generated on 2026-10-20 by `corpus/build.py` on a single Linux build box
(x86_64 host, Ubuntu 24.04 userland). 57 self-contained seed programs were
compiled across every toolchain and flag combination that machine supports,
producing 849 manifest rows in `dataset/manifest.csv`.

| toolchain | what it actually is | targets covered |
|-----------|--------------------|-----------------|
| `gcc13`   | the system gcc, 13.3.0 | `x86_64-linux-gnu` |
| `zig-cc`  | zig's `cc` driver bundling clang 19.1.7 | `x86_64-linux-gnu`, `x86_64-linux-musl` (static), `aarch64-linux-gnu`, `x86_64-windows-gnu` |

The extractor is ELF-only, so the 98 `x86_64-windows-gnu` PE rows are
recorded in the manifest with ground truth but are not part of the feature
corpus or the model today. Training uses the remaining 751 binaries; their
label distributions:

| label | values (count) |
|-------|----------------|
| `compiler_family` | gcc (392), clang (359) |
| `compiler_version` | 13.3.0 (392), 19.1.7 (359) |
| `opt_level` | O0 (241), O1 (65), O2 (257), O3 (65), Os (74), Oz (49) |
| `arch` | x86_64 (653), aarch64 (98) |
| `os` | linux (751) |
| `abi` | gnu (653), musl (98) |
| `pie` | no-pie (434), pie (317) |
| `lto` | off (703), on (48) |
| `static` | no (629), yes (122) |

Several axes are entangled in this seed: every aarch64 build is also `no-pie`,
every musl build is also `x86_64` and `static`, gcc is the only toolchain that
produced O1, O3, and Os, and zig is the only one that produced Oz. Labels come
from the build matrix, not from inspection, so these correlations are
properties of the matrix, not noise in the labels.

## How the model is trained

One multinomial logistic regression per label field
(`LogisticRegression(max_iter=3000, C=4.0)`), fitted on L2-normalized integer
count vectors. The raw corpus in `dataset/features/train.jsonl` holds
895,084 distinct features; the trainer keeps features seen in at least 150
binaries (`--min-df 150`), which leaves a 6,253-feature vocabulary and a
1.4 MB `model/model.json` while losing nothing measurable on the hold-out
(thresholds from 40 to 150 score within a point of each other). Each field
uses a stratified 80/20 train/test split (`random_state=42`, 601 train /
151 test). Feature weights are rounded to 6 significant digits at export,
and the metrics below are computed with the rounded weights, so they
describe the file that ships, not an idealized copy of it.

## Results

Held-out metrics per field. `os` is not modeled: the ELF-only corpus contains
a single class (`linux`), and the trainer skips fields with fewer than two
classes.

| field | classes | top-1 | top-3 | macro-F1 |
|-------|---------|-------|-------|----------|
| compiler_family | 2 | 1.0000 | 1.0000 | 1.0000 |
| compiler_version | 2 | 1.0000 | 1.0000 | 1.0000 |
| opt_level | 6 | 0.8411 | 0.9934 | 0.7795 |
| arch | 2 | 1.0000 | 1.0000 | 1.0000 |
| os | 1 | - | - | not modeled |
| abi | 2 | 1.0000 | 1.0000 | 1.0000 |
| pie | 2 | 1.0000 | 1.0000 | 1.0000 |
| lto | 2 | 0.9338 | 1.0000 | 0.4829 |
| static | 2 | 0.9934 | 1.0000 | 0.9878 |

The perfect fields deserve suspicion rather than celebration: with one gcc
and one clang in the matrix, family, version, architecture, ABI, and PIE are
close to deterministic functions of very visible features (comment-section
tokens, section layout, register profile). The interesting field is
`opt_level`, where the model must grade code style rather than pick a
toolchain.

`opt_level` per class on the hold-out:

| class | precision | recall | F1 | test support |
|-------|-----------|--------|----|--------------|
| O0 | 1.000 | 0.938 | 0.968 | 48 |
| O1 | 0.929 | 1.000 | 0.963 | 13 |
| O2 | 0.738 | 0.865 | 0.796 | 52 |
| O3 | 0.200 | 0.077 | 0.111 | 13 |
| Os | 0.812 | 0.867 | 0.839 | 15 |
| Oz | 1.000 | 1.000 | 1.000 | 10 |

The O3 row is the honest weak point: eleven of thirteen O3 binaries are
called O2, which matches how gcc behaves on small programs - at these sizes
O3 rarely emits anything O2 would not. `opt_level` top-1 of 0.8411 is well
above the majority baseline (always answering O2 scores 0.342), and top-3 of
0.9934 means the true level is almost always on the podium.

`lto` is the other cautionary row: top-1 0.9338 equals the majority baseline
(always answering `off` scores 0.936), and macro-F1 0.4829 reveals why - with
only 48 `on` rows in 751, the model learns to say `off` and almost never
commits to `on`. The `static` field clears its baseline (0.9934 vs 0.837)
despite a similar imbalance, because static linking leaves loud structural
fingerprints; LTO on tiny programs does not.

## Reproducing

From the repository root:

```sh
cargo build --release

python3 -m venv .venv
.venv/bin/pip install -r training/requirements.txt

# extract + train + evaluate + write model/model.json (~2 minutes)
.venv/bin/python training/train.py
```

The trainer prints the full evaluation report and writes
`dataset/features/train.jsonl` and `model/model.json`. Useful variations:

```sh
.venv/bin/python training/train.py --extract-only          # refresh the feature corpus
.venv/bin/python training/train.py --features-in dataset/features/train.jsonl --min-df 75
.venv/bin/python training/train.py --limit 60              # quick smoke run
```

Sanity checks against the committed model:

```sh
./target/release/compid dataset-info
./target/release/compid classify dataset/binaries/atoi_manual-gcc13-x86_64-linux-gnu-O0-nopie-nolto-dyn
./target/release/compid explain dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn
```

An end-to-end gate run on 2026-10-20 classified three manifest binaries
(x86_64 gcc O0, x86_64 clang O2, aarch64 clang O2) and got the correct top-1
for `compiler_family` and `opt_level` on all three, with softmax
probabilities between 0.85 and 0.99.

## Limitations

- **One machine, two compilers.** Every row comes from a single build box
  running one gcc (13.3.0) and one clang (19.1.7, via zig). A binary from gcc
  14, an MSVC build, or a different clang will get confident wrong answers:
  the softmax has no "none of the above" class.
- **Entangled axes.** As listed above, architecture, ABI, PIE, and some
  optimization levels are correlated with toolchain in this seed. The perfect
  per-field scores partly reflect that structure, and they will separate only
  when the matrix grows (for example aarch64 with PIE, or clang at O1).
- **Class imbalance.** LTO-on rows are 6.4% of the corpus and the `lto`
  field sits at its majority baseline. More LTO builds - or class-weighted
  training - is the obvious next step; the trainer currently uses plain
  LogisticRegression so its numbers stay easy to reproduce.
- **`compiler_version` mirrors `compiler_family`.** Each family appears with
  exactly one version, so the version field is a re-spelling of the family
  field. It will become a real prediction only with multiple versions of one
  compiler in the matrix.
- **Windows is recorded but not modeled.** The 98 `x86_64-windows-gnu` rows
  exist in the manifest and carry ground truth; the ELF-only extractor skips
  them. A PE/COFF extractor would add the `windows` class to `os` and make
  the `pie`/`static` `n/a` values meaningful.
- **Stripped-input caveat.** Evaluation binaries keep their sections;
  comment tokens (a strong family signal) vanish in stripped builds, which
  the fixture set covers but this hold-out does not isolate.
