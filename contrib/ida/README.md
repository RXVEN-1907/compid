# compid IDA plugin

An IDAPython plugin that annotates the current IDB with
[compid](https://github.com/RXVEN-1907/compid) provenance predictions. It
registers two actions under **Edit > Plugins**:

- **compid: classify provenance** (`Ctrl-Alt-C`) - runs
  `compid classify --json` on the input file and appends a plate comment
  with one line per predicted field (toolchain, OS, hardening, ...) to the
  function at the entry point, or to the extra comment lines at the segment
  start when the entry point has no function.
- **compid: function provenance** (`Ctrl-Alt-F`) - runs
  `compid functions --json` and appends a one-line prediction (compiler
  family, optimization level) to every function whose start address matches
  a row of compid's output.

Errors (compid missing, non-zero exit, unparseable JSON) show up as warning
dialogs; the plugin never modifies anything when compid fails.

## Install

1. Build and install compid (`cargo install --path /path/to/compid`), or set
   `COMPID_BIN` to the absolute path of the binary.
2. Copy `compid_ida.py` to `%IDAUSR%/plugins` (on Windows:
   `%APPDATA%\Hex-Rays\IDA Pro\plugins`) and restart IDA. To try it without
   installing, run the file once as a script (Alt-F7): it registers the
   actions and runs classify immediately. The hotkeys can be changed in
   Options > Shortcuts.

## Environment variables

- `COMPID_BIN` - absolute path to the compid binary; defaults to `compid`
  on the PATH IDA inherits.
- `COMPID_MODEL` - absolute path to a trained model file for `classify`
  (the repository's `model/model.json` works). compid otherwise looks for
  `model/model.json` relative to its working directory, which under IDA is
  usually not a compid checkout, so set this in practice.
- `COMPID_FUNC_MODEL` - absolute path to a function-level model
  (`model/functions.json`) for per-function predictions; without one the
  functions action reports that no model was in use.

## Status

Experimental - written against the IDAPython APIs of IDA 7.x/8.x/9.x and
tested with IDA 8/9 patterns, but not exercised inside IDA by the compid
author; report issues at the compid repository. The Ghidra script
(`ghidra_scripts/compid_classify.py`) is the reference implementation and
covers the same workflow on the Ghidra side.
