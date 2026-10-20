# Annotate the current IDB with compid provenance predictions.
#
# IDA plugin: Edit > Plugins > "compid: classify provenance" / "compid: function provenance"

"""compid_ida.py - provenance predictions as IDA comments.

Runs the compid binary (https://github.com/RXVEN-1907/compid) against the
input file the current IDB was loaded from, parses compid's JSON output,
and writes the results into the database:

  - classify: appends a plate comment with one line per predicted field to
    the function at the entry point, or to the extra comment lines at the
    segment start when the entry point has no function.
  - functions: appends a one-line per-function prediction (compiler family
    and optimization level) to every function whose start address matches a
    row of `compid functions --json`.

Two actions are registered under Edit > Plugins, each with a hotkey.

Install:

  1. Build and install compid somewhere IDA can run it, e.g.
     `cargo install --path /path/to/compid`. If it is not on the PATH IDA
     inherits, set the COMPID_BIN environment variable (or the constant
     below) to its absolute path.
  2. Copy this file to the plugins directory (%IDAUSR%/plugins, e.g.
     %APPDATA%\\Hex-Rays\\IDA Pro\\plugins on Windows) and restart IDA.
     To try it without installing, run it once as a script (Alt-F7): it
     registers the actions and runs classify immediately.
  3. `compid classify` needs a trained model. By default compid looks for
     model/model.json relative to its working directory, which under IDA is
     usually not a compid checkout - set COMPID_MODEL to the absolute path
     of a trained model file (the repository's model/model.json works).
     For per-function predictions set COMPID_FUNC_MODEL to a function-level
     model (the repository's model/functions.json); without one the
     functions action can only report that no model was in use.

Notes:

  - The input file must still exist on disk at the path the IDB recorded
    at load time (the plugin tells you when it does not).
  - Function addresses are the file's virtual addresses as compid prints
    them; an IDB that was rebased away from the file's image base will
    match few or no functions.
  - Experimental: written against the IDAPython APIs of IDA 7.x, 8.x and
    9.x but never exercised inside IDA by the compid author. The Ghidra
    script (ghidra_scripts/compid_classify.py) is the reference
    implementation; if this plugin misbehaves, report it at the compid
    repository.
"""

COMPID_BIN = "compid"
COMPID_MODEL = ""
COMPID_FUNC_MODEL = ""

HOTKEY_CLASSIFY = "Ctrl-Alt-C"
HOTKEY_FUNCTIONS = "Ctrl-Alt-F"

import json
import os
import subprocess
import sys

import ida_entry
import ida_kernwin
import idaapi

ACTION_CLASSIFY = "compid:classify"
ACTION_FUNCTIONS = "compid:functions"
MENU_PATH = "Edit/Plugins/"
MARKER = "compid provenance predictions:"
# Extra comment lines above an item occupy consecutive slots from 0; the
# plugin only manages the contiguous run that starts at slot 0.
MAX_EXTRA_SLOTS = 32


def _env(name, fallback):
    """Environment variable override for a script-top constant."""
    value = os.environ.get(name)
    return value if value else fallback


def _text(b):
    """subprocess pipes are bytes under Python 3 and str under Python 2;
    normalize so the rest of the script need not care."""
    if sys.version_info[0] >= 3 and isinstance(b, bytes):
        return b.decode("utf-8", "replace")
    return b


def warn(msg):
    ida_kernwin.warning("compid: %s\n" % msg)


def find_compid():
    """Return the compid executable path, or None after warning why not."""
    path = _env("COMPID_BIN", COMPID_BIN)
    if os.sep in path or (os.altsep and os.altsep in path):
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
        warn("COMPID_BIN is '%s', which is missing or not executable. Fix the "
             "environment variable or the constant at the top of this plugin." % path)
        return None
    for directory in os.environ.get("PATH", "").split(os.pathsep):
        if not directory:
            continue
        candidate = os.path.join(directory, path)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    warn("the compid binary was not found on PATH (COMPID_BIN='%s'). Install it "
         "with `cargo install --path <compid checkout>` first, or set COMPID_BIN "
         "to the absolute path of the compid binary." % path)
    return None


def input_binary_path():
    """Return the IDB's recorded input file path, or None after warning why not."""
    path = idaapi.get_input_file_path()
    if not path or not path.strip():
        warn("this IDB has no recorded input file path, so there is nothing to "
             "classify.")
        return None
    if not os.path.isfile(path):
        warn("the input file recorded for this IDB is not on disk:\n  %s\n"
             "put it back at that path (or re-load the binary) and run again." % path)
        return None
    return path


def run_json(argv):
    """Run compid and parse its JSON stdout.

    Returns (data, None) on success or (None, message) after the failure has
    already been shown; never raises."""
    try:
        proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        out, err = proc.communicate()
    except OSError as e:
        warn("could not execute '%s': %s" % (argv[0], e))
        return None, "oserror"
    out, err = _text(out), _text(err)
    if proc.returncode != 0:
        detail = err.strip() or out.strip()
        if len(detail) > 500:
            detail = detail[:500] + "..."
        warn("'%s' failed (exit code %d).\n%s" % (" ".join(argv), proc.returncode, detail))
        return None, "exit"
    if not out.strip():
        warn("'%s' produced no output; nothing to record." % " ".join(argv))
        return None, "empty"
    try:
        return json.loads(out), None
    except ValueError as e:
        warn("could not parse the JSON output of '%s': %s\n"
             "the compid output format may have changed since this plugin was "
             "written; output starts with: %.200r" % (" ".join(argv), e, out))
        return None, "json"


def _pct(probability):
    """A 0..1 probability as a fixed-percent string, '?' when absent."""
    if isinstance(probability, (int, float)):
        return "%.2f%%" % (100.0 * probability)
    return "?"


def prediction_lines(fields):
    """The plate block, one line per predicted field."""
    lines = [MARKER]
    for f in fields:
        cls = f.get("predicted")
        top3 = f.get("top3") or []
        prob = top3[0].get("probability") if top3 else None
        if not cls and top3:
            cls = top3[0].get("class")
        name = f.get("field") or "?"
        if cls is None:
            lines.append("  %s: (no prediction)" % name)
            continue
        line = "  %s: %s (%s)" % (name, cls, _pct(prob))
        if f.get("low_confidence"):
            line += " (low confidence)"
        lines.append(line)
    return lines


def _merge_block(existing, block_lines):
    """Existing comment text plus our block, replacing a block from a previous
    run so re-running never stacks duplicates."""
    kept = []
    if existing:
        for line in existing.splitlines():
            if line.strip() == MARKER:
                break
            kept.append(line)
    base = "\n".join(kept).rstrip("\n")
    block = "\n".join(block_lines)
    return base + "\n\n" + block if base else block


def entry_ea():
    """The program entry point address, or idaapi.BADADDR."""
    try:
        ordinal = ida_entry.get_entry_ordinal(0)
        if ordinal:
            ea = ida_entry.get_entry(ordinal)
            if ea is not None and ea != idaapi.BADADDR:
                return ea
    except Exception:
        pass
    return idaapi.BADADDR


def append_func_line(func, text):
    """Append one line to a function's non-repeatable comment, skipping a
    line that is already there so re-running stays idempotent."""
    existing = idaapi.get_func_cmt(func, False) or ""
    if text in existing.splitlines():
        return
    updated = existing.rstrip("\n") + "\n" + text if existing else text
    idaapi.set_func_cmt(func, updated, False)


def extra_lines_at(ea):
    """The extra comment lines above an item, as a list."""
    lines = []
    for slot in range(MAX_EXTRA_SLOTS):
        line = idaapi.get_extra_cmt(ea, slot)
        if not line:
            break
        lines.append(line)
    return lines


def annotate_classify(fields):
    """Write the plate block at the entry point's function comment, or at the
    segment start as extra comment lines. Returns where it landed, or None."""
    block = prediction_lines(fields)
    ea = entry_ea()
    if ea != idaapi.BADADDR:
        func = idaapi.get_func(ea)
        if func is not None:
            existing = idaapi.get_func_cmt(func, False)
            idaapi.set_func_cmt(func, _merge_block(existing, block), False)
            return "the function comment at 0x%x (entry point)" % ea
        seg = idaapi.getseg(ea)
    else:
        seg = idaapi.get_first_seg()
    if seg is None:
        return None
    start = seg.start_ea
    # Slots 0.. run above the item; rewrite the contiguous run, keeping any
    # user lines that precede our block and dropping the block it replaces.
    existing = extra_lines_at(start)
    kept = []
    for line in existing:
        if line.strip() == MARKER:
            break
        kept.append(line)
    updated = kept + block
    for slot, line in enumerate(updated):
        idaapi.set_extra_cmt(start, slot, line)
    for slot in range(len(updated), len(existing)):
        idaapi.set_extra_cmt(start, slot, "")
    return "the extra comments at 0x%x (segment start)" % start


def run_classify():
    compid = find_compid()
    if compid is None:
        return
    bin_path = input_binary_path()
    if bin_path is None:
        return
    argv = [compid, "classify"]
    model = _env("COMPID_MODEL", COMPID_MODEL)
    if model:
        argv += ["--model", model]
    argv.append(bin_path)
    print("compid: running: %s" % " ".join(argv))
    data, _ = run_json(argv)
    if data is None:
        return
    fields = data.get("fields") if isinstance(data, dict) else None
    if not fields:
        warn("no predictions in the classify output; the compid output format "
             "may have changed since this plugin was written.")
        return
    where = annotate_classify(fields)
    if where is None:
        warn("the program has no segments; nothing to annotate.")
        return
    for line in prediction_lines(fields):
        print("compid: %s" % line)
    print("compid: appended to %s." % where)


def run_functions():
    compid = find_compid()
    if compid is None:
        return
    bin_path = input_binary_path()
    if bin_path is None:
        return
    argv = [compid, "functions", "--json"]
    func_model = _env("COMPID_FUNC_MODEL", COMPID_FUNC_MODEL)
    if func_model:
        argv += ["--model", func_model]
    argv.append(bin_path)
    print("compid: running: %s" % " ".join(argv))
    data, _ = run_json(argv)
    if data is None:
        return
    rows = data.get("functions") if isinstance(data, dict) else None
    if rows is None:
        warn("no function list in the functions output; the compid output "
             "format may have changed since this plugin was written.")
        return
    if not rows:
        reason = data.get("reason")
        warn("compid found no functions: %s" % (reason or "no functions"))
        return
    if data.get("model") is None:
        print("compid: no function model in use; set COMPID_FUNC_MODEL (or run "
              "compid from a checkout with model/functions.json) to get "
              "per-function predictions.")
    matched = 0
    missed = 0
    for row in rows:
        family = row.get("compiler_family") or {}
        opt = row.get("opt_level") or {}
        parts = []
        if family.get("predicted"):
            parts.append("%s (%s)" % (family["predicted"], _pct(family.get("confidence"))))
        if opt.get("predicted"):
            parts.append("%s (%s)" % (opt["predicted"], _pct(opt.get("confidence"))))
        if not parts:
            continue
        try:
            ea = int(str(row.get("addr", "")), 16)
        except ValueError:
            continue
        func = idaapi.get_func(ea)
        if func is None:
            missed += 1
            continue
        append_func_line(func, "compid: " + ", ".join(parts))
        matched += 1
    if matched == 0 and missed > 0:
        print("compid: no function start addresses matched this IDB; if the "
              "program was rebased, matching needs the file's image base.")
    print("compid: annotated %d functions (%d prediction rows matched no "
          "function)." % (matched, missed))


class ClassifyHandler(idaapi.action_handler_t):
    def activate(self, ctx):
        run_classify()
        return 1

    def update(self, ctx):
        return idaapi.AST_ENABLE_ALWAYS


class FunctionsHandler(idaapi.action_handler_t):
    def activate(self, ctx):
        run_functions()
        return 1

    def update(self, ctx):
        return idaapi.AST_ENABLE_ALWAYS


def register_actions():
    """(Re)register the two actions and their menu entries; the unregister
    first makes a script re-run idempotent."""
    for name, label, hotkey, handler in (
        (ACTION_CLASSIFY, "compid: classify provenance", HOTKEY_CLASSIFY,
         ClassifyHandler()),
        (ACTION_FUNCTIONS, "compid: function provenance", HOTKEY_FUNCTIONS,
         FunctionsHandler()),
    ):
        idaapi.unregister_action(name)
        desc = idaapi.action_desc_t(name, label, handler, hotkey, label)
        if not idaapi.register_action(desc):
            print("compid: could not register action '%s'" % name)
            continue
        idaapi.attach_action_to_menu(MENU_PATH, name, idaapi.SETMENU_APP)


class CompidPlugin(idaapi.plugin_t):
    # PLUGIN_FIX: load once and keep the actions alive for the session.
    # PLUGIN_HIDE: the plugin itself stays out of Edit > Plugins; the two
    # registered actions are the interface.
    flags = idaapi.PLUGIN_FIX | idaapi.PLUGIN_HIDE
    comment = "compid provenance predictions"
    help = "Runs the compid binary on the input file; see contrib/ida/README.md"
    wanted_name = "compid"
    wanted_hotkey = ""

    def init(self):
        register_actions()
        return idaapi.PLUGIN_OK

    def run(self, arg):
        run_classify()

    def term(self):
        pass


def PLUGIN_ENTRY():
    return CompidPlugin()


# Script mode (Alt-F7): PLUGIN_ENTRY is never called outside the plugin
# loader, so register here and run the primary action once.
if __name__ == "__main__":
    register_actions()
    run_classify()
