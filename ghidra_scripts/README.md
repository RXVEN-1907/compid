# Ghidra scripts

`compid_classify.py` connects a Ghidra Code Browser session to the compid
CLI. With a program open, the script runs `compid classify` against the
binary the program was imported from, prints the full classification table
to the script console, and appends one line per predicted field -
`compiler_family: gcc (99.96%)`, `opt_level: O0 (85.12%)`, and so on - to
the plate comment at the lowest address of the listing, where it stays
visible at the top of the Code Browser. The annotation is a normal Ghidra
comment: it participates in undo, and saving the program keeps it.

The script never modifies bytes; it only adds a comment.

## Requirements

- Ghidra with its bundled Jython interpreter (the default for `.py`
  scripts). No extra Ghidra plugins or Python packages.
- The compid binary, installed from this repository (`cargo install
  --path .`), with a trained model available - the committed
  `model/model.json` works.
- The source binary on disk at the path Ghidra recorded at import time.
  Ghidra projects keep a reference to the original file; if the file was
  moved or deleted, the script says so and stops.

## Install

1. Open the Script Manager in Ghidra (Window -> Script Manager).
2. Click "Manage Script Directories" and add or enable a personal folder,
   for example `~/ghidra_scripts`.
3. Copy `compid_classify.py` into that folder, press Refresh in the Script
   Manager, and the script appears under `Analysis.compid`.
4. Run it with a program open. Assign a key binding in the Script Manager
   if you plan to triage samples with it regularly.

## Configuration

Two constants sit at the top of the script:

| constant       | default    | meaning                                              |
|----------------|------------|------------------------------------------------------|
| `COMPID_PATH`  | `compid`   | path of the compid binary; set an absolute path when it is not on the PATH Ghidra inherits |
| `COMPID_MODEL` | (empty)    | path of a trained model file; when empty, compid uses its own default (`model/model.json` relative to its working directory), so set this when Ghidra's working directory is not the repository root |

## Behavior details

- The plate comment is written at the program's minimum address (the
  header region), which renders at the very top of the listing. An
  existing plate comment there is preserved and the predictions are
  appended after a blank line.
- The prediction lines carry the model path and training date from the
  classify output, so the annotation records which model produced it.
- Failure modes are reported in the console and leave the program
  untouched: compid not installed, source binary missing, model missing,
  or a container format the installed compid cannot read.
- The script targets Jython (Python 2.7 syntax); it also normalizes
  subprocess output so a Python 3 runtime works.

## See also

- `docs/cli.md` for the `compid classify` output the script parses.
- `docs/model-format.md` for the model file the script depends on.
- `docs/roadmap.md` for where the Ghidra integration is heading (deeper
  UI integration and an IDA equivalent are still open).
