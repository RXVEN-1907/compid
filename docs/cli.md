# CLI reference

compid ships as one binary with eight subcommands plus a hidden completions
helper. `compid --help` and `compid <subcommand> --help` describe the exact
CLI of the installed build; this page explains the shape and the defaults
behind them.

```text
compid [--version] [--help] <subcommand> [args]
```

Defaults that matter:

- `--model` defaults to `model/model.json` relative to the repository root;
  pass it explicitly when classifying from elsewhere. The per-function
  `functions` subcommand defaults to `model/functions.json` instead.
- `--manifest` on `dataset-info` defaults to `dataset/manifest.csv`.

## compid extract

```text
compid extract <BIN> [-o out.json]
```

Prints the feature JSON for one binary to stdout, or writes it to `out.json`
with `-o`. The JSON follows the schema in `docs/dataset.md`: `build_id` and
`labels` are empty strings for binaries outside the corpus, and `features`
holds the integer counts over `.text`.

ELF, PE (Windows, x86-64), and Mach-O (macOS, x86-64 and arm64) binaries
extract through this one command; the container is detected from the file's
magic bytes. PE output carries `os=windows`, the architecture spelled from
the COFF machine (`x86_64`), and the manifest's windows-row conventions for
the fields a PE does not express (`pie`/`static` = `n/a`); the import
table's DLL names appear as lowercase `cmt:` features. Mach-O output
carries `os=macos`, the architecture spelled from the header cputype
(`x86_64`, `aarch64`), `pie=pie` (modern macOS executables are PIE by
construction), and the dylib load-command names as lowercase basename
`cmt:` features; 32-bit Mach-O images are rejected with a clear error.

## compid classify

```text
compid classify <BIN> [--model PATH] [--json]
```

Loads the model, extracts features from `<BIN>`, and prints, for each label
field the model knows, up to three classes with their probabilities:

```text
$ compid classify /tmp/sample
/tmp/sample  (ELF64 x86-64, PIE, dynamically linked)

compiler_family   clang          0.71
                  gcc            0.19
opt_level         O2             0.64
                  O3             0.21
                  Os             0.08
...
```

`--json` swaps the table for a machine-readable object: one entry per field,
each a map of class name to probability. Consumers should ignore unknown
fields; the schemas version themselves (`format`, `feature_version`).

PE and Mach-O binaries classify through the same command. A field whose
classes the loaded model does not know (say, a `macos` value before a
retrain that includes macos rows) simply never comes up: the model answers
only from the classes it was trained on, and the extractor reports what it
sees.

## compid explain

```text
compid explain <BIN> [--model PATH]
```

For the top predicted class of each field, lists the features that
contributed most to that score (weight times normalized tf), plus the
notable contributions pulling toward the runner-up. Plainly human-readable
on purpose; the JSON form of the same data is `compid extract` plus the
model, so explain stays a reading tool.

## compid compare

```text
compid compare <A> <B>
```

Prints the cosine similarity between the L2-normalized feature vectors of
binaries `A` and `B`. `1.0` means identical fingerprints - comparing a file
with itself is the trivially correct case; values near `0` mean unrelated
builds. Useful against a reference build you trust: a sharp drop in
similarity after a toolchain bump is exactly the kind of change this catches.

## compid model-info

    compid model-info [--model PATH]

Describe a trained model: format, the header line classify uses, every
field with its classes, and the provenance block (manifest row count and
sha256, feature row count and sha256) when the trainer recorded one.

## compid dataset-info

```text
compid dataset-info [--manifest PATH]
```

Reads the manifest CSV and prints, per label field, the count of rows for
each label value, along with the total row count and how many duplicate
SHA-256 rows were found. This is the quickest way to see what a dataset
actually contains before trusting anything trained on it.

## compid batch

```text
compid batch <MANIFEST> [--out FILE]
```

Runs extraction for every manifest row whose binary exists. Feature objects
are emitted as JSON lines - one per binary - to stdout, or appended to
`FILE` with `--out`. Rows whose binary is missing are skipped and summarized
in a final line, so a partial local checkout still processes everything
present.

## compid functions

```text
compid functions <BIN> [--model PATH] [--features FILE] [--json]
```

Per-function provenance for one binary. Function boundaries come from the
ELF symbol table, so the input must be an unstripped ELF - a relocatable
`.o` works too. The table lists each function's raw (never demangled)
symbol name, its address as the symbol table records it, its size, and -
when a function-level model is loaded - the predicted `compiler_family`
and `opt_level` with per-function confidence, plus a footer tallying the
predicted families:

```text
$ compid functions mixed-demo --model model/functions.json
binary: mixed-demo
build-id: 3da973efdbc7bd7b221629e265070e0501902e8d
model:   model/functions.json (trained 2026-10-20, 20669 features)

name                      addr  size  compiler_family        opt_level
main                    0x1070   184  gcc          1.00   O2           0.88
_start                  0x1130    38  gcc          1.00   O2           0.55
zig_half_main           0x1c10   218  clang        1.00   O0           0.93
op_inc                  0x1cf0     9  clang        1.00   O0           1.00

11 functions; predicted families: clang=6, gcc=5
```

A binary with no usable functions (a stripped ELF, or a PE/Mach-O) prints
the reason instead of a table and exits 0. `--model` defaults to
`model/functions.json` when that file exists; an explicitly passed `--model`
that does not exist is an error. A model file carries only the two
per-function fields, so the table never predicts container-level labels -
see `docs/FUNCTIONS-EVALUATION.md` for the per-function hold-out numbers.

`--features FILE` writes the per-function training JSONL instead of the
table: one object per function with `build_id`, `name`, `addr` (hex
string), `size`, `labels` (the extractor's own best-effort container
labels), and `features`. `--json` swaps the table for a machine-readable
object with per-function predictions and the family tally.

`scripts/mixed_demo.sh` runs the end-to-end proof: two corpus sources
compiled with gcc and zig cc, linked into one binary, with the assertion
that each half's functions majority-predict their own toolchain.

## compid functions-batch

```text
compid functions-batch <MANIFEST> [--out FILE] [--features]
```

Runs per-function extraction over every manifest row whose binary is an
unstripped ELF. With `--features`, writes the per-function training JSONL -
one object per function with `build_id`, `name`, `addr`, `size`, `labels`
(the manifest's ground-truth labels for that row, the same convention as
`batch`), and `features` - which is the input `training/train_functions.py`
expects. Without `--features`, writes a plain per-binary listing (name,
address, size). Rows whose binary is missing, stripped, or not an ELF are
skipped with a per-row stderr note and summarized in the final line.

```console
$ compid functions-batch dataset/manifest.csv --features --out dataset/features/functions.jsonl
```

The functions JSONL is a regeneration artifact like `train.jsonl`: rebuild
it from the manifest and binaries rather than committing it.

## compid completions

```text
compid completions <shell>
```

Prints a shell completion script to stdout for one of `bash`, `zsh`,
`fish`, or `powershell` - no flags, just the script. The subcommand is
hidden from `--help` because it is a one-time install helper, not a daily
driver. Install the script where your shell looks for completions:

- bash: `compid completions bash > ~/.local/share/bash-completion/completions/compid`
  (create the directory first if it does not exist; the completion loads
  on the next interactive shell)
- zsh: `compid completions zsh > ~/.zfunc/_compid` and make sure
  `~/.zfunc` is in your `fpath` before `compinit` runs
- fish: `compid completions fish > ~/.config/fish/completions/compid.fish`
- PowerShell: `compid completions powershell >> $PROFILE` (or dot-source
  the output from your profile)

The generated script covers the subcommands and flags of the build that
produced it, so regenerate it after upgrading compid. Unknown shell names
are rejected with a usage error (exit code 2) listing the supported ones.

## Exit codes

| code | meaning                                              |
|------|------------------------------------------------------|
| 0    | success                                              |
| 1    | runtime error (unreadable binary, missing model, ...) |
| 2    | usage error (unknown flag, missing argument)          |

## Scripting notes

- Output for a given (binary, model, feature schema) triple is deterministic,
  so outputs are diffable.
- Non-UTF-8 input paths are fine; binaries are bytes.
- Every subcommand's exact flags come from `--help` on your installed
  version - this page describes behavior, `--help` is the contract for
  spelling.
