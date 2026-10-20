#!/usr/bin/env python3
"""compid function-level trainer: fit per-function classifiers and export model/functions.json.

Input is the per-function JSONL written by `compid functions-batch
<MANIFEST> --features --out FILE` (or `compid functions <BIN> --features
FILE`): one JSON object per function, with `build_id`, `name`, `addr`,
`size`, `labels`, and `features`.

Two things differ from the whole-binary trainer (train.py):

1. The hold-out split is BY BUILD_ID. Every function of one binary lands on
   the same side of the split. Functions carved from one binary are
   near-duplicates of their neighbors - same section padding, same literal
   pools, same per-build idiosyncrasies - so a per-function random split
   would leak a build's functions into its own hold-out and inflate every
   number. The split shuffles whole builds (stratified by the build's
   compiler_family when every family owns at least two builds) and the
   metrics are computed over the held-out builds' functions.

2. Only compiler_family and opt_level are trained. Those two travel with
   the code generator and the flags; the other seven manifest fields
   (arch, os, abi, pie, lto, static, compiler_version) describe the
   container or the whole link, which a single function does not witness.

The recipe otherwise mirrors train.py: per-field multinomial logistic
regression (LogisticRegression(max_iter=3000, C=4.0)) over L2-normalized
integer counts, features seen in fewer than --min-df functions dropped.
The floor is lower than the binary trainer's 150 because functions are
small: rare n-grams survive less often in any single function. Export is
model/functions.json in the same compid-model/v1 format the binary model
uses, so the Rust reader (src/model.rs) loads it unchanged - only the
field set is smaller.

The script also writes docs/FUNCTIONS-EVALUATION.md with the dataset card,
the per-field hold-out table, and the split rationale, so the reported
numbers and their provenance regenerate together.
"""

from __future__ import annotations

import argparse
import gzip
import json
import sys
import warnings
from collections import Counter
from datetime import date
from pathlib import Path

import numpy as np
from sklearn.exceptions import ConvergenceWarning
from sklearn.feature_extraction import DictVectorizer
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import f1_score
from sklearn.model_selection import train_test_split
from sklearn.preprocessing import normalize

# Reuse the whole-binary trainer's helpers rather than forking them: the
# rounding, pruning, softmax, and ranking helpers are contractually the same
# on both paths.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from train import (  # noqa: E402
    FEATURE_VERSION,
    MODEL_FORMAT,
    log,
    prune_by_document_frequency,
    round6,
    softmax,
    topk_accuracy,
)

REPO_ROOT = Path(__file__).resolve().parent.parent

MODEL_OUT = REPO_ROOT / "model" / "functions.json"
EVAL_OUT = REPO_ROOT / "docs" / "FUNCTIONS-EVALUATION.md"

DEFAULT_FIELDS = ["compiler_family", "opt_level"]


def load_functions_jsonl(path: Path) -> list[dict]:
    records = []
    with path.open(encoding="utf-8") as f:
        for lineno, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as e:
                log(f"warning: skipping unparseable line {lineno}: {e}")
                continue
            for key in ("build_id", "name", "labels", "features"):
                if key not in rec:
                    log(f"warning: line {lineno} lacks '{key}'; skipping")
                    break
            else:
                records.append(rec)
    return records


def split_by_build(
    records: list[dict], field: str, test_size: float, random_state: int
) -> tuple[set[str], set[str]]:
    """Shuffle whole build_ids into train/hold-out, stratified by the builds'
    value of `field` when every class owns at least two builds."""
    builds = sorted({rec["build_id"] for rec in records})
    build_value = {}
    for rec in records:
        build_value.setdefault(rec["build_id"], rec["labels"][field])
    strata = [build_value[b] for b in builds]
    try:
        train_builds, test_builds = train_test_split(
            builds, test_size=test_size, random_state=random_state, stratify=strata
        )
    except ValueError:
        log(
            "  build split: a class has fewer than 2 builds; "
            "falling back to an unstratified build split"
        )
        train_builds, test_builds = train_test_split(
            builds, test_size=test_size, random_state=random_state
        )
    return set(train_builds), set(test_builds)


def fit_and_export(
    field: str, dicts: list[dict], y: list[str], idx_train: list[int], idx_test: list[int], args
) -> dict | None:
    """Train one field on the pre-split rows; export rounded weights plus
    held-out metrics computed with those weights."""
    vec = DictVectorizer(sparse=True)
    x_all = vec.fit_transform(dicts)
    x_all = normalize(x_all, norm="l2", copy=False)

    classes = sorted(set(y))
    if len(classes) < 2:
        log(f"field {field}: only {classes} present -> skipped (needs 2+ classes)")
        return None
    class_index = {c: i for i, c in enumerate(classes)}
    y_idx = np.array([class_index[v] for v in y])

    # Function rows skew heavily toward the matrix's dominant toolchain
    # (clang rows outnumber gcc ~2.5:1 in the current seed). Unweighted
    # training let the prior swallow minority-toolchain functions in
    # mixed binaries, so the fit is class-balanced; the whole-binary
    # trainer keeps unweighted fits because its classes are near-even.
    weights = None if getattr(args, "no_balanced", False) else "balanced"

    clf = LogisticRegression(max_iter=args.max_iter, C=args.C, class_weight=weights)
    with warnings.catch_warnings():
        warnings.simplefilter("error", ConvergenceWarning)
        try:
            clf.fit(x_all[idx_train], y_idx[idx_train])
        except ConvergenceWarning:
            log(f"  {field}: hit max_iter; keeping the fit and continuing")
            clf = LogisticRegression(max_iter=args.max_iter, C=args.C, class_weight=weights)
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ConvergenceWarning)
                clf.fit(x_all[idx_train], y_idx[idx_train])

    # Export with the rounded weights the Rust reader will actually use.
    # Binary problems get the [+w, -w] mirror (see train.py for why the
    # argmax and the reported probabilities stay consistent).
    if len(classes) == 2:
        wr = [round6(v) for v in np.asarray(clf.coef_[0], dtype=np.float64)]
        b = round6(float(clf.intercept_[0]))
        coef = np.array([[round6(-v) for v in wr], wr])
        intercept = np.array([-b if b != 0.0 else 0.0, b])
    else:
        coef = np.array(
            [[round6(v) for v in row] for row in np.asarray(clf.coef_, dtype=np.float64)]
        )
        intercept = np.array([round6(v) for v in np.asarray(clf.intercept_, dtype=np.float64)])

    def evaluate(idx: list[int]) -> dict:
        x = x_all[idx]
        scores = np.asarray(x.dot(coef.T)) + intercept
        probs = softmax(scores)
        yt = y_idx[idx]
        pred = probs.argmax(axis=1)
        top1 = float(np.mean(pred == yt))
        macro_f1 = float(
            f1_score(yt, pred, labels=list(range(len(classes))), average="macro", zero_division=0)
        )
        top3 = topk_accuracy(probs, yt, 3)
        return {"top1": top1, "top3": top3, "macro_f1": macro_f1, "pred": pred, "probs": probs}

    test = evaluate(idx_test)
    train = evaluate(idx_train)
    return {
        "field": field,
        "classes": classes,
        "features": list(vec.get_feature_names_out()),
        "intercept": [float(v) for v in intercept],
        "coef": [[float(v) for v in row] for row in coef],
        "n_functions": len(y),
        "n_train": len(idx_train),
        "n_test": len(idx_test),
        "test": test,
        "train": train,
    }


def build_majority_top1(
    field: str,
    records: list[dict],
    idx_test: list[int],
    pred_names: list[str],
) -> float:
    """Majority vote over each held-out build's functions, scored against the
    build's own label. This is the number the mixed-toolchain use case cares
    about: per-function votes pooled per binary. Ties break to the class
    that sorts first, matching the deterministic argmax convention."""
    build_true: dict[str, str] = {}
    build_votes: dict[str, Counter] = {}
    for i, idx in enumerate(idx_test):
        b = records[idx]["build_id"]
        build_true.setdefault(b, records[idx]["labels"][field])
        build_votes.setdefault(b, Counter())[pred_names[i]] += 1
    if not build_votes:
        return 0.0
    hits = 0
    for b, votes in build_votes.items():
        winner = min(votes.items(), key=lambda kv: (-kv[1], kv[0]))[0]
        hits += int(winner == build_true[b])
    return hits / len(build_votes)


def dataset_card(records: list[dict]) -> dict:
    builds: dict[str, dict] = {}
    for rec in records:
        b = builds.setdefault(rec["build_id"], {"labels": rec["labels"], "functions": 0})
        b["functions"] += 1
    return {
        "n_functions": len(records),
        "n_builds": len(builds),
        "families": Counter(b["labels"]["compiler_family"] for b in builds.values()),
        "family_functions": Counter(rec["labels"]["compiler_family"] for rec in records),
        "opts": Counter(b["labels"]["opt_level"] for b in builds.values()),
        "archs": Counter(b["labels"]["arch"] for b in builds.values()),
        "oses": Counter(b["labels"]["os"] for b in builds.values()),
        "os_functions": Counter(rec["labels"]["os"] for rec in records),
    }


def write_eval_doc(path: Path, card: dict, rows: list[dict], args, skipped: list[str]) -> None:
    def pct(x: float) -> str:
        return f"{x:.4f}"

    lines: list[str] = []
    lines.append("# Function-level evaluation\n")
    lines.append(
        "This page reports how the committed per-function model "
        "(`model/functions.json`) performs on builds it was not fitted on, "
        "how that hold-out was chosen, and what the numbers do and do not "
        "cover. It is generated by `training/train_functions.py` at the same "
        "time as the model, so the numbers and the file ship together.\n"
    )

    lines.append("## The dataset\n")
    lines.append(
        f"Per-function features extracted by `compid functions-batch --features` "
        f"from **{card['n_builds']} builds with extractable functions** "
        f"({card['n_functions']} functions): ELF rows via the symbol table "
        "(or the stripped-x86-64 prologue heuristic), Mach-O rows via "
        "`LC_SYMTAB`, and PE rows via the export directory. Every function "
        "inherits its build's manifest labels; nobody labels functions by "
        "hand.\n"
    )
    lines.append("| axis | builds (functions) |")
    lines.append("|------|--------------------|")
    fam_sorted = sorted(card["families"].items())
    lines.append(
        "| compiler_family | "
        + ", ".join(f"{k} ({card['family_functions'][k]} fns)" for k, _ in fam_sorted)
        + " |"
    )
    lines.append(
        "| opt_level | "
        + ", ".join(f"{k} ({n})" for k, n in sorted(card["opts"].items()))
        + " |"
    )
    lines.append(
        "| arch | " + ", ".join(f"{k} ({n})" for k, n in sorted(card["archs"].items())) + " |"
    )
    lines.append(
        "| os | "
        + ", ".join(
            f"{k} ({card['os_functions'].get(k, 0)} fns)" for k, _ in sorted(card["oses"].items())
        )
        + " |"
    )
    lines.append("")

    lines.append("## The hold-out is split by build, not by function\n")
    lines.append(
        f"The {card['n_functions']} functions are not independent rows: the "
        "functions of one binary share padding, literal pools, and every "
        "per-build idiosyncrasy, so a per-function random split would leak a "
        "build into its own hold-out. The split shuffles whole build_ids "
        f"({args.test_size:.0%} of builds held out, seed {args.random_state}, "
        "stratified by the build's compiler_family) and every metric below is "
        "computed over the held-out builds' functions. All functions of a "
        "build sit on exactly one side of the split.\n"
    )

    lines.append("## Results (hold-out builds, per function)\n")
    lines.append("| field | classes | functions | top-1 | top-3 | macro-F1 | build majority top-1 |")
    lines.append("|-------|---------|-----------|-------|-------|----------|----------------------|")
    for row in rows:
        t = row["test"]
        lines.append(
            f"| `{row['field']}` | {len(row['classes'])} | {row['n_test']} | "
            f"{pct(t['top1'])} | {pct(t['top3'])} | {pct(t['macro_f1'])} | "
            f"{pct(row['build_majority'])} |"
        )
    for name in skipped:
        lines.append(f"| `{name}` | - | - | - | - | - | - |")
    if skipped:
        lines.append("")
        lines.append(
            "Skipped fields (present in the manifest labels but with fewer "
            "than two classes in this corpus): " + ", ".join(f"`{s}`" for s in skipped) + "."
        )
    lines.append("")
    lines.append(
        "The *build majority* column pools each held-out build's per-function "
        "votes and scores the winning class against the build's label - the "
        "operation `scripts/mixed_demo.sh` performs on a binary assembled "
        "from two toolchains. Per-function top-1 is the harder, unaggregated "
        "number.\n"
    )

    weak = [
        row
        for row in rows
        if row["test"]["macro_f1"] < row["test"]["top1"] - 0.15
    ]
    if weak:
        names = ", ".join(f"`{row['field']}`" for row in weak)
        lines.append("## Reading the weak spots honestly\n")
        lines.append(
            f"{names} lags badly on macro-F1 relative to top-1, which is the "
            "signature of class imbalance plus sibling-class confusion: "
            "optimization levels shade into one another (O2 and O3 emit the "
            "same shapes for most functions), and small classes have almost "
            "no functions to learn from at function granularity.\n"
        )
        lines.append(
            "There is also structural label noise the whole-binary trainer "
            "never faces. Every function inherits its build's labels, so the "
            "thousands of libc functions inside a statically linked binary "
            "are labeled with the container's optimization level even though "
            "libc itself was compiled once, at its own flags, by whatever "
            "built the sysroot. Those functions dominate the function count "
            "of static builds and drag the per-function metrics for "
            "`opt_level` in particular; the per-function `compiler_family` "
            "numbers survive the same noise because the sysroot is still "
            "built by a member of the same family in this seed (zig compiles "
            "its bundled musl with clang).\n"
        )

    lines.append("## How the model is trained\n")
    weighting = (
        "unweighted (`--no-balanced`)"
        if getattr(args, "no_balanced", False)
        else "class_weight='balanced'"
    )
    lines.append(
        "One multinomial logistic regression per field "
        f"(`LogisticRegression(max_iter={args.max_iter}, C={args.C}, "
        f"{weighting})`) over L2-normalized integer counts - the "
        "whole-binary recipe plus class weighting, because function rows "
        "inherit the matrix's toolchain skew (clang rows outnumber gcc "
        "roughly 2.5:1) and an unweighted fit let the prior swallow "
        "minority-toolchain functions inside mixed binaries. Disable with "
        f"`--no-balanced`. Features seen in fewer than {args.min_df} "
        "functions are dropped (the whole-binary trainer's floor is 150; "
        "functions are small, so rare n-grams survive less often). The "
        "export is `compid-model/v1` - the same format as "
        "`model/model.json` - with only `compiler_family` and `opt_level` "
        "fields.\n"
    )

    lines.append("## Stripped-binary boundaries — measured, not promised\n")
    lines.append(
        "`compid functions` no longer refuses a stripped binary outright. "
        "For x86-64 ELF with no symbol table, two conservative scans over "
        "the executable sections propose function starts, and their union "
        "is merged by the same decode-aware overlap logic; extraction "
        "proceeds with synthesized `func_<hex addr>` names, marked "
        "`source: heuristic` in the JSON and by a header note in the table. "
        "Nothing is guessed for other architectures: a stripped non-x86-64 "
        "ELF, a PE without an export table, and a Mach-O without a symbol "
        "table still report their explicit reasons.\n"
    )
    lines.append(
        "The first scan is the prologue-pattern set (one const array in "
        "`src/functions.rs`): `push rbp; mov rbp, rsp` and `push rbx; mov "
        "rbx, rdi` at 16-byte alignment, `endbr64` at any alignment with a "
        "frame-setup followup — and, at aligned sites, none — plus the "
        "paired-`lea` crt-stub shape and PLT-stub exclusions. The second "
        "scan is direct call-target analysis, which catches functions that "
        "lack a frame-setup prologue (cold clones, shrink-wrapped `-Os` "
        "entries): with the same length-aware x86-64 decoder the "
        "clean-window count uses, each executable section is disassembled "
        "linearly (skipping undecodable bytes one at a time), and every "
        "decoded direct `call rel32` (opcode `E8`) records the target "
        "`instruction address + instruction length + signed displacement` "
        "as a candidate start. A call target qualifies when it falls inside "
        "the scanned section (cross-section branches, e.g. into `.plt`, are "
        "thereby excluded, matching the prologue scan's PLT treatment), "
        "decodes cleanly for at least 16 bytes from the target, does not "
        "open with a PLT-stub shape (`endbr64; jmp *GOT`, bare `jmp *GOT` / "
        "`push *GOT`, or `push imm32; jmp rel32`), and is not already a "
        "prologue candidate. Direct `jmp rel32` (`E9`) targets were "
        "measured too and stay out: they add tail-thunk entries but far "
        "more loop back-edges and goto joins, which split otherwise-correct "
        "functions — with `E9` included the same sweep lands at precision "
        "0.4334 / recall 0.9215 (F1 0.5896). The choice is pinned in one "
        "const (`CALL_TARGETS_INCLUDE_JMP`) in `src/functions.rs`, and the "
        "whole candidate union runs through the same merge pipeline, so a "
        "start found only by a call target is indistinguishable from a "
        "prologue start.\n"
    )
    lines.append(
        "The heuristic is measured, not asserted. `compid functions <bin> "
        "--measure` runs the scan on an unstripped binary and scores its "
        "starts against the symbol table's starts at a ±8-byte tolerance; "
        "`scripts/measure_boundaries.py` sweeps a stratified sample over "
        "the manifest. The numbers below are the sweep over 40 linux-gnu "
        "x86_64 manifest rows (8 per optimization level, O0 through Os, "
        "seed 42), before and after the call-target scan joined the "
        "prologue scan — re-run `scripts/measure_boundaries.py` against "
        "any manifest to reproduce the shape of this table:\n"
    )
    lines.append("| pooled over 40 binaries | prologue scan only | prologue + call targets |")
    lines.append("|-------------------------|--------------------|-------------------------|")
    lines.append("| symbol-table starts (ground truth) | 2713 | 2713 |")
    lines.append("| heuristic starts | 2376 | 2535 |")
    lines.append("| matched within ±8 bytes | 2330 | 2489 |")
    lines.append("| precision | 0.9806 | 0.9819 |")
    lines.append("| recall | 0.8588 | 0.9174 |")
    lines.append("| F1 | 0.9157 | 0.9486 |")
    lines.append("")
    lines.append(
        "The call-target scan buys 159 recovered ground-truth starts while "
        "adding only 13 unmatched starts — precision ticks up, not down, "
        "because a direct `call` is strong evidence of a real entry point. "
        "Per optimization level, pooled precision stays at 0.96-1.00 "
        "everywhere and pooled recall is 0.75-0.95 (O0 0.92, O1 0.76, O2 "
        "0.95, O3 0.75, Os 0.75). Eight further rows were skipped because "
        "their symbol tables carry no function symbols at all (zig-cc "
        "builds that list only undefined imports) — no ground truth, no "
        "measurement.\n"
    )
    lines.append("What the residual misses are, honestly:\n")
    lines.append(
        "- Functions smaller than the 16-byte minimum: `.fini` (13 bytes) "
        "on every dynamic binary, and any function abutting its section's "
        "end."
    )
    lines.append(
        "- Functions never reached by a direct `call` and matching no "
        "prologue pattern: entries reached only through indirect dispatch "
        "(function pointers, vtables) and never-called leftovers."
    )
    lines.append(
        "- These cost recall, not precision: across the sweep, wrong "
        "starts are rare (pooled precision 0.98), and the ones that occur "
        "sit at legitimate-looking prologues or called addresses in "
        "padding.\n"
    )
    lines.append(
        "Recall is bounded, and that is the point of the conservative set: "
        "a missed function costs one vote in the per-binary majority, while "
        "a false start injects a vote for whatever the padding decodes as. "
        "The stripped mixed-toolchain demo (`scripts/mixed_demo.sh`, second "
        "half) runs the same gcc/zig majority assertion on a `strip`ped "
        "copy and passes: the gcc half majority-predicts gcc and the zig "
        "half majority-predicts clang (5 of its 6 attributed functions, "
        "with 16 heuristic functions on the stripped copy against 11 before "
        "the call-target scan).\n"
    )

    lines.append("## Limitations\n")
    lines.append(
        "- Function boundaries come from the container's symbol sources: the "
        "ELF symbol table (plus the stripped-x86-64 prologue heuristic), the "
        "PE export directory, and the Mach-O `LC_SYMTAB`. What yields no "
        "function rows: a PE without an export table (most plain Windows "
        "executables), a Mach-O without a symbol table, and a stripped "
        "non-x86-64 ELF. The PE rows are export-table functions only - the "
        "bulk of a DLL's internal (non-exported) functions is invisible to "
        "this path, and the last export of a section carries a "
        "section-end-derived size rather than a true one.\n"
        "- Every function of a build shares the build's labels. A function "
        "inlined from a vendored, differently-built library is labeled with "
        "its container's toolchain, not its true generator; the mixed-demo "
        "split is the sanity check that the model reads code shape, not "
        "container residue (functions carry no `sec:`, `ent:`, or `cmt:` "
        "features).\n"
        "- Same-toolchain-version bias as the whole-binary seed: a toolchain "
        "the matrix never saw will get confident wrong answers.\n"
        "- Small functions (under a few dozen bytes) carry little signal; "
        "their votes are near-noise, which is one reason the majority vote "
        "matters more than any single function's confidence.\n"
    )

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")
    log(f"wrote {path.relative_to(REPO_ROOT)}")


def main() -> int:
    ap = argparse.ArgumentParser(
        prog="train_functions.py",
        description="Train per-function provenance classifiers from a functions JSONL "
        "and write model/functions.json (compid-model/v1).",
    )
    ap.add_argument("--features-in", type=Path, required=True,
                    help="functions JSONL from `compid functions-batch --features` (or `compid functions --features`)")
    ap.add_argument("--model-out", type=Path, default=MODEL_OUT, help="model JSON output path")
    ap.add_argument("--eval-out", type=Path, default=EVAL_OUT,
                    help="evaluation markdown output path (pass 'none' to skip)")
    ap.add_argument("--fields", default=",".join(DEFAULT_FIELDS),
                    help="comma-separated fields to train (default: %(default)s; "
                         "per-function records only train container-independent fields)")
    ap.add_argument("--min-df", type=int, default=50, metavar="N",
                    help="keep only features present in at least N functions "
                         "(default: %(default)s; lower than the binary trainer's 150 "
                         "because functions are small)")
    ap.add_argument("--test-size", type=float, default=0.2, help="hold-out fraction of BUILDS (default: %(default)s)")
    ap.add_argument("--random-state", type=int, default=42, help="split seed (default: %(default)s)")
    ap.add_argument("--max-iter", type=int, default=3000, help="LogisticRegression max_iter (default: %(default)s)")
    ap.add_argument("--C", type=float, default=4.0, help="LogisticRegression inverse regularization (default: %(default)s)")
    ap.add_argument("--no-balanced", action="store_true", help="disable class-weighted fitting (default: class_weight=balanced)")
    args = ap.parse_args()

    records = load_functions_jsonl(args.features_in)
    if len(records) < 10:
        log(f"error: too few function rows ({len(records)}) in {args.features_in} to train on")
        return 1
    n_builds = len({r["build_id"] for r in records})
    log(f"loaded {len(records)} functions from {n_builds} builds ({args.features_in})")

    dicts = [rec["features"] for rec in records]
    dicts = prune_by_document_frequency(dicts, args.min_df)
    vocab = Counter(k for d in dicts for k in d)
    log(f"vocabulary: {len(vocab)} features after min-df={args.min_df} pruning")
    by_prefix = Counter(name.split(":", 1)[0] for name in vocab)
    log("  by prefix: " + ", ".join(f"{p}={n}" for p, n in sorted(by_prefix.items())))

    # The build split happens once (stratified by compiler_family - the
    # headline axis) and every field trains against the same side, so the
    # hold-out never contains one build's functions for one field and not
    # for another.
    train_builds, test_builds = split_by_build(
        records, "compiler_family", args.test_size, args.random_state
    )
    idx_train = [i for i, r in enumerate(records) if r["build_id"] in train_builds]
    idx_test = [i for i, r in enumerate(records) if r["build_id"] in test_builds]
    log(
        f"build split: {len(train_builds)} training builds ({len(idx_train)} functions), "
        f"{len(test_builds)} hold-out builds ({len(idx_test)} functions)"
    )

    wanted = [f.strip() for f in args.fields.split(",") if f.strip()]
    fields_payload: dict[str, dict] = {}
    rows: list[dict] = []
    skipped: list[str] = []

    for field in wanted:
        y = [rec["labels"][field] for rec in records]
        payload = fit_and_export(field, dicts, y, idx_train, idx_test, args)
        if payload is None:
            skipped.append(field)
            continue
        fields_payload[field] = {
            "classes": payload["classes"],
            "intercept": payload["intercept"],
            "coef": payload["coef"],
        }
        t = payload["test"]
        pred_names = [payload["classes"][p] for p in t["pred"]]
        build_majority = build_majority_top1(field, records, idx_test, pred_names)
        log(
            f"  {field}: hold-out top-1 {t['top1']:.4f}  top-3 {t['top3']:.4f}  "
            f"macro-F1 {t['macro_f1']:.4f}  build-majority top-1 {build_majority:.4f}  "
            f"(train top-1 {payload['train']['top1']:.4f}, {payload['n_test']} held-out functions)"
        )
        rows.append({**payload, "build_majority": build_majority})

    if not fields_payload:
        log("error: no field trained; nothing to export")
        return 1

    model = {
        "format": MODEL_FORMAT,
        "feature_version": FEATURE_VERSION,
        "created": date.today().isoformat(),
        "features": sorted(vocab),
        "fields": {name: fields_payload[name] for name in sorted(fields_payload)},
    }
    args.model_out.parent.mkdir(parents=True, exist_ok=True)
    with args.model_out.open("w", encoding="utf-8") as f:
        json.dump(model, f, separators=(",", ":"))
    # A compressed copy ships embedded in the CLI binary (src/model.rs
    # include_bytes!); written with no gzip timestamp so rebuilds stay
    # byte-reproducible.
    gz_path = args.model_out.with_suffix(args.model_out.suffix + ".gz")
    with gz_path.open("wb") as raw, gzip.GzipFile(
        fileobj=raw, mode="wb", filename="", mtime=0, compresslevel=9
    ) as gz:
        gz.write(args.model_out.read_bytes())
    size_kb = args.model_out.stat().st_size / 1024
    log(
        f"wrote {args.model_out} ({size_kb:.0f} KiB, {len(model['features'])} features, "
        f"{len(fields_payload)} fields)"
    )

    log("\nevaluation report (hold-out builds, split seed %d):" % args.random_state)
    log(
        f"{'field':<18} {'classes':>7} {'fns':>6} {'top-1':>7} {'top-3':>7} "
        f"{'macro-F1':>9} {'build-maj':>10}"
    )
    for row in rows:
        t = row["test"]
        log(
            f"{row['field']:<18} {len(row['classes']):>7} {row['n_functions']:>6} "
            f"{t['top1']:>7.4f} {t['top3']:>7.4f} {t['macro_f1']:>9.4f} "
            f"{row['build_majority']:>10.4f}"
        )
    for name in skipped:
        log(f"{name:<18} {'-':>7} {'-':>6}   skipped (needs 2+ classes)")

    if str(args.eval_out).lower() != "none":
        write_eval_doc(args.eval_out, dataset_card(records), rows, args, skipped)
    return 0


if __name__ == "__main__":
    sys.exit(main())
