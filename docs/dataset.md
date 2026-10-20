# The dataset

Everything compid knows, it learned from a dataset it builds itself. This
document specifies the on-disk formats - the manifest, the per-binary feature
records - and how the matrix grows. The builder that produces all of it lives
in `corpus/` and is documented in `corpus/README.md`.

## Files

| path                        | committed                 | contents                                              |
|-----------------------------|---------------------------|-------------------------------------------------------|
| `dataset/manifest.csv`      | yes                       | one row per built binary, with ground-truth labels    |
| `dataset/binaries/`         | no (gitignored)           | the binaries themselves; manifest paths point here    |
| `dataset/features/train.jsonl.gz.*` | yes, after a training run | extracted feature JSON, one object per line, gzipped, split into <100 MB shards |
| `dataset/features/train.jsonl` | no (gitignored)        | the uncompressed same thing; a regeneration artifact  |

Reassemble the features file from the shipped shards with:

```console
$ cat dataset/features/train.jsonl.gz.* | gunzip > dataset/features/train.jsonl
```

Regenerate it from the binaries any time with:

```console
$ cargo build --release
$ ./target/release/compid batch dataset/manifest.csv --out dataset/features/train.jsonl
```

## Manifest schema

`dataset/manifest.csv` is a UTF-8 CSV whose header is exactly:

```csv
build_id,source_id,toolchain,compiler_family,compiler_version,opt_level,arch,os,abi,pie,lto,static,path,sha256
```

| column            | meaning                                                                 |
|-------------------|-------------------------------------------------------------------------|
| `build_id`        | unique, stable id for this build; feature files are keyed by it         |
| `source_id`       | which seed source program was compiled                                  |
| `toolchain`       | short toolchain id used for the build (e.g. `gcc13`, `zig-cc`)          |
| `compiler_family` | ground truth for the `compiler_family` label (`gcc`, `clang`, ...)      |
| `compiler_version`| ground-truth compiler version (e.g. `13.3.0`)                           |
| `opt_level`       | ground-truth optimization level (`O0`, `O1`, `O2`, `O3`, `Os`, ...)     |
| `arch`            | target architecture (`x86-64`, `aarch64`, ...)                          |
| `os`              | target OS (`linux`, ...)                                                |
| `abi`             | libc / ABI flavor (`gnu`, `musl`, ...)                                  |
| `pie`             | PIE on/off at build time (`yes` / `no`)                                 |
| `lto`             | LTO on/off at build time (`yes` / `no`)                                 |
| `static`          | static linking on/off at build time (`yes` / `no`)                      |
| `path`            | binary path relative to the repository root (`dataset/binaries/...`)    |
| `sha256`          | lowercase hex SHA-256 of the binary file                                |

Rules:

- One row per binary; `build_id` is unique across the file.
- Labels record what the build command asked for, not what anyone observed in
  the resulting binary. Ground truth comes from the build matrix, never from
  inspection.
- `sha256` pins the row to content. If a build is ever regenerated with
  different bytes, it gets a fresh `build_id`; row identity follows content,
  not intent.

## Feature record schema

One JSON object per binary:

```json
{
  "build_id": "seed-loop-0017",
  "labels": {
    "compiler_family": "clang",
    "compiler_version": "19.1.5",
    "opt_level": "O2",
    "arch": "x86-64",
    "os": "linux",
    "abi": "gnu",
    "pie": "yes",
    "lto": "no",
    "static": "no"
  },
  "features": {
    "ng:554889e5": 214,
    "mn:mov": 1203,
    "rg:rax": 806,
    "cn:00000000": 351,
    "sec:text": 13,
    "ent:text": 6,
    "meta:windows": 4021,
    "cmt:clang": 1
  }
}
```

Feature keys are `<prefix>:<name>`; every value is an integer count over the
`.text` section:

| prefix         | value type    | meaning                                                                 |
|----------------|---------------|-------------------------------------------------------------------------|
| `ng:`          | count         | three-byte n-grams of `.text`, hex-encoded (e.g. `ng:554889e5`)         |
| `mn:`          | count         | disassembled mnemonics (`mn:mov`)                                       |
| `rg:`          | count         | register mentions in the disassembly (`rg:rax`)                         |
| `cn:`          | count         | 32-bit immediates, lowercase hex, no `0x` prefix (`cn:00000000`)        |
| `sec:`         | log2 size     | log2 of the section's size in bytes (`sec:text` = 13 is 8 KiB)          |
| `ent:`         | integer 0-8   | the section's byte entropy in bits per symbol, rounded (`ent:text`)     |
| `meta:windows` | count         | 64-byte sliding windows of `.text` that disassemble cleanly             |
| `meta:macos`   | count         | the same count over a Mach-O image's `__TEXT,__text`; the macOS container marker |
| `cmt:`         | count         | tokens from comment and note string sections (`cmt:clang`)              |

Notes:

- Counts are raw integer tf values. No normalization happens in the JSON;
  L2 normalization is applied identically at train time and at infer time.
- Section keys spell the name the way the container does: ELF sections are
  bare names (`sec:text`), PE sections carry their COFF spelling
  (`sec:.text`), and Mach-O sections pair segment and section name
  (`sec:__TEXT,__text`), since Mach-O section names are only unique within
  their segment.
- `labels` carries the ground truth recorded by the corpus builder. For a
  binary extracted outside the corpus - `compid extract` on a sample of your
  own - every label is an empty string. Training reads labels from the
  manifest, never from the feature files.
- `build_id` is the empty string for binaries extracted outside the corpus,
  and matches the manifest for corpus binaries.

## The current seed matrix

The seed matrix is whatever the build machine could compile on the day of the
run. On the machine that produced the current dataset that meant:

- the system gcc (13.x, native `x86_64-linux-gnu`), and
- a zig `cc` driver (a clang-family code generator) covering
  `x86_64-linux-gnu`, `x86_64-linux-musl`, `aarch64-linux-gnu`, and
  `x86_64-windows-gnu` targets when zig is available,

across the flag axes: `-O0`..`-O3` and `-Os`, PIE on/off, LTO on/off, static
versus dynamic linking, gnu versus musl libc. The committed
`dataset/manifest.csv` is the authoritative record of what actually exists;
`corpus/README.md` documents how toolchains are discovered and how to point
the builder at a zig install (`COMPID_ZIG`).

Sizes are modest and that is fine: the pipeline is the product. A dataset
that grows by re-running a documented command beats a frozen archive every
time.

## Expanding the matrix

All expansion is adding rows, and every path converges on the same recipe.

1. **New seed source.** Add a small, self-contained program to the corpus
   sources. It is built across every existing toolchain and flag combination
   automatically.
2. **New toolchain.** Put it on the build machine and teach the discovery
   module (`corpus/toolchains.py`) how to probe it. Everything downstream -
   matrix, manifest, training - picks it up unchanged.
3. **New flag axis.** Teach the builder the axis (for example
   `-fstack-protector-all`, `-march=`, CFI flags). The axis multiplies rows
   across all existing sources and toolchains, which is the cheapest way to
   grow the dataset.

After any expansion: rebuild the corpus, re-extract, retrain, and commit the
refreshed manifest, features, and model together - a model and its dataset
move as a pair.

## Hygiene

- Deduplicate by `sha256`; identical bytes are one row, not two.
- Nothing vendored. Every binary in the dataset is built from a seed source
  in this repository, which keeps licensing clean and provenance exact.
- Record toolchain versions exactly as the discovery module reports them.
  "clang-ish" is not a label.
