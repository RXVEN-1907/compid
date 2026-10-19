# Annotate the current Ghidra program with compid provenance predictions.
#
#@category: Analysis.compid
#@runtime: Jython

"""compid_classify.py - provenance predictions as a listing plate comment.

Runs the compid binary (https://github.com/RXVEN-1907/compid) against the
source binary the current program was imported from, parses the
`compid classify` table, appends one line per predicted field to the plate
comment at the lowest address of the program, and prints the full table to
the script console.

Install:

  1. Build and install compid somewhere Ghidra can run it, e.g.
     `cargo install --path /path/to/compid`. If it is not on the PATH
     Ghidra inherits, set COMPID_PATH below to its absolute path.
  2. In Ghidra, open the Script Manager (Window -> Script Manager), click
     "Manage Script Directories", and add (or enable) a personal folder
     such as ~/ghidra_scripts. Copy this file there, click Refresh, and the
     script shows up under Analysis.compid. Run it with a program open,
     from the Script Manager or a key binding you assign there.
  3. `compid classify` needs a trained model. By default compid looks for
     model/model.json relative to its working directory; if that does not
     suit your setup, set COMPID_MODEL below to the absolute path of a
     trained model file (the repository's model/model.json works).

Notes:

  - The source binary must still exist on disk at the path Ghidra recorded
    at import time (the script tells you when it does not).
  - The installed compid decides which container formats it can read. When
    it cannot parse the binary, the script prints compid's own error and
    leaves the program untouched.
"""

COMPID_PATH = "compid"
COMPID_MODEL = ""

import os
import re
import subprocess
import sys

from ghidra.program.model.listing import CodeUnit


RANK_RE = re.compile(r"^\s+(\d)\.\s+(\S+)\s+(\d+\.\d+)%")
MODEL_RE = re.compile(r"^model:\s+(.*?)\s+\(trained\s+([^)]*)\)")


def _text(b):
    """subprocess pipes are bytes under Python 3 (PyGhidra) and str under
    Jython; normalize so the rest of the script need not care."""
    if sys.version_info[0] >= 3 and isinstance(b, bytes):
        return b.decode("utf-8", "replace")
    return b


def find_compid():
    """Return the compid executable path, or None after printing why not."""
    path = COMPID_PATH
    if os.sep in path or (os.altsep and os.altsep in path):
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
        println("compid: COMPID_PATH is '%s', which is missing or not executable." % path)
        println("compid: fix the constant at the top of this script and run it again.")
        return None
    for directory in os.environ.get("PATH", "").split(os.pathsep):
        if not directory:
            continue
        candidate = os.path.join(directory, path)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
    println("compid: the compid binary was not found on PATH (COMPID_PATH='%s')." % path)
    println("compid: install it with `cargo install --path <compid checkout>` first,")
    println("compid: or set COMPID_PATH at the top of this script to the absolute")
    println("compid: path of the compid binary, then run this script again.")
    return None


def source_binary_path():
    """Return the imported binary's path, or None after printing why not."""
    path = currentProgram.getExecutablePath()
    if path is None or path.strip() == "":
        println("compid: this program has no recorded source binary path, so")
        println("compid: there is nothing to classify. Re-import the binary")
        println("compid: with the regular importer and run the script there.")
        return None
    if not os.path.isfile(path):
        println("compid: the source binary recorded for this program is not on disk:")
        println("compid:   %s" % path)
        println("compid: put it back at that path (or re-import it) and run again.")
        return None
    return path


def run_compid(compid, bin_path):
    """Run compid classify; return (returncode, stdout, stderr) or None."""
    argv = [compid, "classify"]
    if COMPID_MODEL:
        argv += ["--model", COMPID_MODEL]
    argv.append(bin_path)
    println("compid: running: %s" % " ".join(argv))
    try:
        proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        out, err = proc.communicate()
    except OSError as e:
        println("compid: could not execute '%s': %s" % (compid, e))
        println("compid: check COMPID_PATH at the top of this script.")
        return None
    return proc.returncode, _text(out), _text(err)


def parse_table(text):
    """Parse `compid classify` human output.

    Returns (fields, model_desc) where fields is a list of dicts with the
    field name, the predicted class, its confidence, and the full top-3
    list of (class, percent) pairs, and model_desc is a short provenance
    string for the model, or '' when the table omits it.
    """
    fields = []
    current = None
    model_desc = ""
    for line in text.splitlines():
        if not line.strip():
            continue
        if not line[:1].isspace():
            head = line.split(":", 1)[0].strip()
            if head in ("binary", "model", "build-id"):
                m = MODEL_RE.match(line)
                if m is not None:
                    model_desc = " (model: %s, trained %s)" % (m.group(1), m.group(2))
                continue
            current = {"field": line.strip(), "predicted": None,
                       "confidence": None, "top": []}
            fields.append(current)
            continue
        m = RANK_RE.match(line)
        if m is not None and current is not None:
            rank = int(m.group(1))
            cls = m.group(2)
            pct = float(m.group(3))
            current["top"].append((cls, pct))
            if rank == 0:
                current["predicted"] = cls
                current["confidence"] = pct
    return fields, model_desc


def plate_lines(fields, model_desc):
    """The plate comment text, one line per field."""
    lines = ["compid provenance predictions%s:" % model_desc]
    for f in fields:
        cls = f["predicted"]
        if cls is None and f["top"]:
            cls = f["top"][0][0]
        if cls is None:
            lines.append("  %s: (no prediction)" % f["field"])
        else:
            lines.append("  %s: %s (%.2f%%)" % (f["field"], cls, f["confidence"] or 0.0))
    return lines


def append_plate_comment(text):
    """Append the text to the plate comment at the program's lowest address."""
    addr = currentProgram.getMinAddress()
    if addr is None:
        println("compid: the program has no addresses; nothing to annotate.")
        return False
    listing = currentProgram.getListing()
    existing = listing.getComment(addr, CodeUnit.PLATE_COMMENT)
    if existing:
        updated = existing + "\n\n" + text
    else:
        updated = text
    tx = currentProgram.startTransaction("compid classify annotation")
    try:
        listing.setComment(addr, CodeUnit.PLATE_COMMENT, updated)
    except:
        currentProgram.endTransaction(tx, False)
        raise
    currentProgram.endTransaction(tx, True)
    return True


def main():
    compid = find_compid()
    if compid is None:
        return
    bin_path = source_binary_path()
    if bin_path is None:
        return
    result = run_compid(compid, bin_path)
    if result is None:
        return
    code, out, err = result
    if code != 0:
        println("compid: classify failed (exit code %d)." % code)
        for line in err.splitlines():
            println("compid: %s" % line)
        return
    if not out.strip():
        println("compid: classify produced no output; nothing to record.")
        return

    fields, model_desc = parse_table(out)
    if not fields:
        println("compid: no predictions parsed from the table below - the compid")
        println("compid: output format may have changed since this script was written.")
        println(out)
        return

    lines = plate_lines(fields, model_desc)
    println("compid: predictions for %s" % bin_path)
    for line in lines:
        println(line)
    println("compid: full classify table:")
    for line in out.splitlines():
        println("  " + line)

    if append_plate_comment("\n".join(lines)):
        println("compid: appended to the plate comment at %s." % currentProgram.getMinAddress())
        println("compid: (plate comments render at the top of the listing; undo reverts it)")


main()
