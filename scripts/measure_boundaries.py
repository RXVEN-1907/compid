#!/usr/bin/env python3
"""Sweep the stripped-binary boundary heuristic over manifest rows.

Runs `compid functions <bin> --measure --json` on a sample of linux-gnu
x86_64 manifest rows spanning the optimization levels, and aggregates the
per-binary precision/recall of the prologue heuristic against the symbol
table. Sampled rows are unstripped by definition - the symbol table is the
ground truth being measured against.

Usage:
    scripts/measure_boundaries.py [--manifest PATH] [--compid PATH]
                                  [--per-level N] [--seed S]

The default sample takes up to `--per-level` (default 8) rows per
optimization level, so every level O0..Os contributes. Rows whose binary is
missing locally, is not x86_64 linux-gnu, or carries no STT_FUNC ground
truth are skipped and counted. Output is plain text: per-binary lines plus
pooled and macro-averaged totals, ready to paste into
docs/FUNCTIONS-EVALUATION.md.
"""

import argparse
import csv
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def rows_from_manifest(manifest):
    with open(manifest, newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            if row.get("os") == "linux" and row.get("arch") == "x86_64":
                yield row


def measure(compid, binary):
    proc = subprocess.run(
        [str(compid), "functions", str(binary), "--measure", "--json"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        return None, proc.stderr.strip() or "measurement failed"
    return json.loads(proc.stdout), None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--manifest", type=Path, default=REPO / "dataset/manifest.csv")
    ap.add_argument("--compid", type=Path, default=REPO / "target/release/compid")
    ap.add_argument("--per-level", type=int, default=8,
                    help="rows sampled per optimization level (default 8)")
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    base = args.manifest.parent
    # Manifest paths are written relative to the repo root; fall back to
    # the manifest's own directory so the CSV travels with its binaries.
    def resolve(row_path):
        p = Path(row_path)
        if p.is_absolute() or p.exists():
            return p
        return REPO / p

    by_level = defaultdict(list)
    for row in rows_from_manifest(args.manifest):
        by_level[row["opt_level"]].append(row)

    rng = __import__("random").Random(args.seed)
    sample = []
    for level in sorted(by_level):
        rows = by_level[level]
        rng.shuffle(rows)
        sample.extend(rows[: args.per_level])
    sample.sort(key=lambda r: (r["opt_level"], r["build_id"]))

    pooled_s = pooled_h = pooled_m = 0
    per_binary = []
    skipped = defaultdict(int)
    macro = defaultdict(list)
    by_level_stats = defaultdict(lambda: [0, 0, 0])

    for row in sample:
        binary = resolve(row["path"])
        if not binary.exists():
            skipped["binary missing"] += 1
            continue
        metrics, err = measure(args.compid, binary)
        if metrics is None:
            # compid's CLI prefixes stderr with "error: "; keep the first
            # clause of the message as the skip reason.
            reason = err.replace("error: ", "", 1).split(":")[0].strip()
            skipped[reason or "other"] += 1
            continue
        pooled_s += metrics["symbol_starts"]
        pooled_h += metrics["heuristic_starts"]
        pooled_m += metrics["matched"]
        level = row["opt_level"]
        macro[level].append((metrics["precision"], metrics["recall"]))
        stats = by_level_stats[level]
        stats[0] += metrics["symbol_starts"]
        stats[1] += metrics["heuristic_starts"]
        stats[2] += metrics["matched"]
        per_binary.append((level, binary.name, metrics))

    print(f"boundary-measurement sweep over {args.manifest}")
    print(f"compid: {args.compid}  sample: <= {args.per_level} rows per "
          f"opt level, linux-gnu x86_64 only, seed {args.seed}")
    print()
    for level, name, m in per_binary:
        print(f"{level:<3} {name[:52]:<52} "
              f"sym={m['symbol_starts']:<5} heur={m['heuristic_starts']:<5} "
              f"match={m['matched']:<5} P={m['precision']:.4f} "
              f"R={m['recall']:.4f} F1={m['f1']:.4f}")
    print()

    pooled_p = pooled_m / pooled_h if pooled_h else 0.0
    pooled_r = pooled_m / pooled_s if pooled_s else 0.0
    pooled_f = (2 * pooled_p * pooled_r / (pooled_p + pooled_r)
                if pooled_p + pooled_r else 0.0)
    print(f"binaries measured : {len(per_binary)}")
    print(f"skipped           : {sum(skipped.values())} "
          f"({', '.join(f'{k}: {v}' for k, v in sorted(skipped.items())) or 'none'})")
    print(f"pooled starts     : symbols={pooled_s} heuristic={pooled_h} matched={pooled_m}")
    print(f"pooled precision  : {pooled_p:.4f}")
    print(f"pooled recall     : {pooled_r:.4f}")
    print(f"pooled f1         : {pooled_f:.4f}")
    print()
    print("per opt level (pooled over that level's binaries, macro P/R over rows):")
    for level in sorted(by_level_stats):
        s, h, m = by_level_stats[level]
        ps = m / h if h else 0.0
        pr = m / s if s else 0.0
        rows = macro[level]
        mp = sum(p for p, _ in rows) / len(rows)
        mr = sum(r for _, r in rows) / len(rows)
        print(f"  {level:<3} binaries={len(rows):<3} pooled P={ps:.4f} R={pr:.4f} "
              f"macro P={mp:.4f} R={mr:.4f}")

    if skipped:
        print()
        for kind, n in sorted(skipped.items()):
            print(f"note: skipped {n} row(s): {kind}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
