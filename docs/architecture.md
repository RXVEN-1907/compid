# Architecture

compid answers one question - "how was this binary built?" - with a
deliberately small pipeline. Everything runs locally, in two languages chosen
for the job they do: Rust where portability and a tight, dependency-free
runtime matter (extraction and inference), Python where iteration speed
matters (corpus building and training).

## Pipeline at a glance

```mermaid
flowchart LR
    S["Seed sources<br/>small C and C++ programs"] --> B["Build matrix<br/>toolchain x -O level x PIE x LTO x linking x libc"]
    B --> BIN["ELF and PE binaries<br/>dataset/binaries (local only)"]
    BIN --> X["compid extract<br/>goblin + yaxpeax-x86"]
    X --> F["Feature JSON per binary<br/>ng: mn: rg: cn: sec: ent: meta: cmt:"]
    F --> MF["dataset/manifest.csv<br/>ground-truth labels + sha256"]
    MF --> T["Python trainer<br/>L2-normalize, one softmax per field"]
    T --> MOD["model/model.json<br/>compid-model/v1"]
    MOD --> C["compid classify / explain<br/>scores, softmax, top-3"]
    BIN --> CMP["compid compare<br/>cosine similarity"]
```

## Stage 1 - the build matrix

The corpus builder (Python, `corpus/`) first discovers which toolchains the
build machine actually has - the system gcc, and when zig is available, a
zig `cc` driver that extends the matrix with musl, aarch64, and other
cross-targets. It then compiles every seed source under every flag
combination that toolchain supports: optimization levels (`-O0` through
`-O3`, `-Os`), PIE on/off, LTO on/off, static versus dynamic linking, and
libc flavor (gnu, musl). Each binary becomes one row in
`dataset/manifest.csv` with the ground-truth labels recorded straight from
the build command - nobody inspects the binary to guess what built it.

The matrix is seeded, not exhaustive. Whatever this machine could build on
the day of the run is what the dataset holds, and the tooling to widen it is
the same tooling (see `docs/dataset.md`).

## Stage 2 - extraction

`compid extract` (Rust) reads the ELF - or the PE, or the Mach-O - with
[goblin](https://docs.rs/goblin) and disassembles `.text` with
[yaxpeax-x86](https://docs.rs/yaxpeax-x86), emitting one JSON object of
integer counts per binary. The container is detected from the file's magic
bytes; the feature families below come out of any of the formats unchanged:

| prefix         | feature                                          | why it separates builds                                                              |
|----------------|--------------------------------------------------|--------------------------------------------------------------------------------------|
| `ng:`          | three-byte hex n-grams over `.text`              | each code generator has a recognizable byte-level texture                            |
| `mn:`          | mnemonic counts from disassembly                 | `-O0` shuffles through `mov`/`push`; optimized code leans on `lea`, `cmov`, `setcc` |
| `rg:`          | register mention counts                          | register-allocation habits differ per compiler and optimization level                |
| `cn:`          | 32-bit immediates, hex-encoded                   | constants that specific toolchains embed (offsets, sizes, magic numbers)             |
| `sec:`         | log2 of each section's size                      | section layout follows the linker and the flags                                      |
| `ent:`         | per-section entropy, integer 0-8                 | plain code, high-entropy data, or compressed content are easy to tell apart          |
| `meta:windows` | 64-byte sliding windows that disassemble cleanly | aligned, compiler-emitted code disassembles cleanly; packed or mixed content does not |
| `cmt:`         | tokens from comment and note string sections; on PE, import-table DLL names; on Mach-O, dylib load-command names | `.comment` often names the compiler and version outright; imports name the OS runtimes a Windows binary touches, dylibs the ones a macOS binary touches |

PE (Windows, x86-64) rides the same pipeline: the same feature
families come out of the PE `.text` section, and the import table's DLL
names are counted as `cmt:` tokens - the strongest OS/libc signal a Windows
binary carries. Labels follow the manifest's windows-row conventions
(`os=windows`, the COFF machine spelling `x86_64`, `pie`/`static` = `n/a`),
and a non-x86-64 PE, if one ever appears, skips the disassembly features
rather than counting garbage.

Mach-O (macOS, x86-64 and arm64) rides the same pipeline too: sections are
named `__SEGMENT,__section` in the `sec:`/`ent:` keys, the dylib
load-command names become basename `cmt:` tokens, and the clean-window
count files under `meta:macos` - the macOS container marker, mirroring how
`meta:windows` doubles as the PE one. Labels are `os=macos`, the cputype
spelling (`x86_64`, `aarch64`), and `pie=pie` (modern macOS requires
position-independent main executables) with `abi`/`static` = `n/a` like
the windows rows. An arm64 Mach-O skips the x86-64 disassembly features
exactly like a non-x86-64 PE would; its byte n-grams and section features
still flow.

Counts are integer tf values over the `.text` section. The JSON holds no
normalized numbers: L2 normalization happens at train time and at infer time,
where it is used. The exact schema is specified in `docs/dataset.md`.

## Stage 3 - labels

One label field per question, all recorded at build time:

| field              | question                       | typical seed values            |
|--------------------|--------------------------------|--------------------------------|
| `compiler_family`  | which code generator           | `gcc`, `clang`                 |
| `compiler_version` | which version of it            | `13.3.0`, `19.1.5`, ...        |
| `opt_level`        | optimization level             | `O0`, `O1`, `O2`, `O3`, `Os`  |
| `arch`             | target architecture            | `x86-64`, `aarch64`            |
| `os`               | target OS                      | `linux`, `windows`             |
| `abi`              | libc / ABI flavor              | `gnu`, `musl`                  |
| `pie`              | position-independent?          | `yes`, `no`                    |
| `lto`              | link-time optimized?           | `yes`, `no`                    |
| `static`           | statically linked?             | `yes`, `no`                    |

The seed grows with the matrix; a field only gets the classes its dataset
actually contains.

## Stage 4 - training

The Python trainer (`training/`) reads the manifest and the feature files,
L2-normalizes every count vector, and fits one multinomial logistic
regression (softmax) per label field, independently. The choice of a linear
model is not a placeholder for something fancier; it is the point:

- The seed dataset is small - hundreds to low thousands of rows. Linear
  models trained on n-gram counts are hard to overfit at that scale.
- A softmax is trivial to reimplement in Rust: one intercept and one weight
  vector per class. Inference needs no Python, no ONNX runtime, no shared
  library.
- The model is one readable JSON file. You can diff two models and see what
  changed.

## Stage 5 - inference

`compid classify` and `compid explain` extract features the same way, keep
only the feature names the model knows (missing names count as zero),
L2-normalize, and compute for every class `c` of every field:

```text
score(c) = intercept(c) + dot(coef(c), x)
```

A softmax over the scores turns them into per-field probabilities, and the
CLI reports the top-3 classes with their confidence. The exact algorithm,
with the exact `model.json` schema, is `docs/model-format.md`.

`compid compare` skips the model entirely and answers a simpler question -
how similar are these two binaries' fingerprints - with the cosine similarity
of their L2-normalized feature vectors.

## Function-level provenance

Whole-binary features answer "how was this binary built"; large real-world
binaries are often built from several answers at once - a gcc main, a
clang-built vendored library, a musl static libc. `compid functions`
re-runs the feature core at function granularity so each answer can be
found separately:

- **Boundaries.** Function ranges come from two sources. Unstripped ELF
  (and relocatable objects): SHT_SYMTAB entries of type STT_FUNC with a
  nonzero size, deduplicated by `(st_value, st_size)`, resolved to file
  offsets through their containing executable section - in `ET_REL` the
  symbol value is a section offset, in a linked binary it is a virtual
  address. Names stay raw; nothing is demangled. Stripped x86-64 ELF: a
  conservative prologue scan over the executable sections (one const
  pattern array in `src/functions.rs` - frame-pointer prologues, CET
  `endbr64` sites with alignment and PLT-stub gates, the crt-stub
  paired-`lea` shape) proposes the starts, each named `func_<hex addr>`,
  with candidates merged by straight-line decode overlap and functions
  under 16 bytes dropped; the result is marked `source: heuristic`. The
  scan is measured against symbol-table ground truth - `compid functions
  <BIN> --measure` prints per-binary precision/recall, and the pooled
  numbers live in `docs/FUNCTIONS-EVALUATION.md` (pooled 0.98 precision,
  0.86 recall over a 40-binary O0-Os sweep). A stripped non-x86-64 ELF, a
  PE, or a Mach-O yields an empty function list with an explicit reason -
  boundaries are never guessed for architectures the scanner does not
  read. PE/Mach-O symbol sources are the next rounds.
- **Features.** Each function gets the same byte- and code-shape
  namespaces scoped to exactly its own bytes: `ng:`, `mn:`, `rg:`, `cn:`,
  plus a one-hot size bucket `fn:<floor(log2(bytes))>` clamped to 0..=24
  and the container marker `meta:windows` (clean 64-byte windows within
  the function). No `sec:`, `ent:`, or `cmt:` keys - those describe the
  container, and letting them in would let a model read the container
  instead of the code, which is the one failure mode function-level
  classification must not have in a mixed-toolchain binary.
- **Training.** `training/train_functions.py` consumes the per-function
  JSONL (`compid functions-batch --features`) and trains
  `compiler_family` and `opt_level` - the two fields a function actually
  witnesses - with the same multinomial LR recipe, exported as
  `model/functions.json` in the same `compid-model/v1` format. One
  critical difference: the hold-out split is **by build**, not by
  function, because neighboring functions of one binary are near-duplicates
  and a per-function split would leak a build into its own hold-out.
  The honest numbers and the split rationale live in
  `docs/FUNCTIONS-EVALUATION.md`.
- **Inference.** `compid functions <BIN> --model model/functions.json`
  prints the per-function table with per-function confidence, and pools
  nothing: the footer tally is a plain count of predicted families.
  Aggregation - majority vote per function group - is left to the caller;
  `scripts/mixed_demo.sh` is the worked example: two corpus sources built
  with gcc and zig cc, linked into one binary, with the mechanical
  assertion that each half's functions majority-predict their own
  toolchain - run twice, unstripped (attribution by symbol name) and on a
  `strip`ped copy (attribution by address, exercising the prologue
  heuristic).

## On-disk layout

| path                   | committed               | contents                                   |
|------------------------|-------------------------|--------------------------------------------|
| `dataset/manifest.csv` | yes                     | one row per built binary                   |
| `dataset/binaries/`    | no (gitignored)         | the binaries themselves                    |
| `dataset/features/`    | yes, after a training run | extracted feature JSON, one object per line (`train.jsonl`) |
| `model/model.json`     | yes                     | the trained whole-binary model (`compid-model/v1`) |
| `model/functions.json` | yes, after a function-level training run | the trained per-function model (same format, two fields) |

## Design choices worth defending

- **Integer counts in the JSON, normalization at use time.** Integer JSON is
  portable, diffable, and stable across platforms; normalization is a
  one-liner on both sides, done identically by the trainer and the CLI.
- **One model file, no server.** `model.json` is a static asset that ships
  with the repo. Classification is a dot product; a service would be theater.
- **Softmax over something exotic.** With small seed data, a linear model
  gives calibrated-ish probabilities and an honest signal when it is unsure;
  a bigger model would mostly be memorizing the build matrix.
- **Rust for the hot path, Python for the cold path.** The CLI must run
  anywhere, on stripped machines, offline. The trainer runs twice a month on
  a build box and can afford scikit-style iteration.
