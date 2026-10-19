# CLI reference

compid ships as one binary with six subcommands plus a hidden completions
helper. `compid --help` and `compid <subcommand> --help` describe the exact
CLI of the installed build; this page explains the shape and the defaults
behind them.

```text
compid [--version] [--help] <subcommand> [args]
```

Defaults that matter:

- `--model` defaults to `model/model.json` relative to the repository root;
  pass it explicitly when classifying from elsewhere.
- `--manifest` on `dataset-info` defaults to `dataset/manifest.csv`.

## compid extract

```text
compid extract <BIN> [-o out.json]
```

Prints the feature JSON for one binary to stdout, or writes it to `out.json`
with `-o`. The JSON follows the schema in `docs/dataset.md`: `build_id` and
`labels` are empty strings for binaries outside the corpus, and `features`
holds the integer counts over `.text`.

Both ELF and PE (Windows, x86-64) binaries extract through this one
command; the container is detected from the file's magic bytes. PE output
carries `os=windows`, the architecture spelled from the COFF machine
(`x86_64`), and the manifest's windows-row conventions for the fields a PE
does not express (`pie`/`static` = `n/a`); the import table's DLL names
appear as lowercase `cmt:` features.

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

PE binaries classify through the same command. A field the loaded model
does not know (say, `os` before a retrain on windows rows) is simply
absent from the output - the extractor reports what it sees, the model
answers only what it was trained on.

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
