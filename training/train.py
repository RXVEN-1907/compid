#!/usr/bin/env python3
"""compid trainer: fit one linear classifier per label field and export model/model.json.

Pipeline (all paths default to locations inside the repository, so running
`python3 training/train.py` or `make train` from anywhere does the right thing):

1. Read dataset/manifest.csv (ground-truth labels come from the manifest,
   never from the extractor's own observations).
2. For every row whose binary exists, run `compid extract <path>` (building
   the release binary first if it is missing) and collect the feature JSONs
   into dataset/features/train.jsonl. Rows whose binary cannot be extracted -
   the PE/COFF windows targets, for instance, since the extractor is
   ELF-only - are skipped and reported.
3. Per label field with at least two classes: DictVectorizer (features seen
   in fewer than --min-df binaries are dropped), L2 row normalization,
   stratified 80/20 train/test split, LogisticRegression.
4. Export model/model.json (format compid-model/v1). All weights are rounded
   to 6 significant digits, and hold-out metrics are computed with the
   rounded weights, so the reported numbers describe the file that ships.

Binary-class fields get one subtlety: scikit-learn represents a binary
problem as a single weight vector (one row), while the Rust reader applies a
softmax over one score row per class. The export mirrors the single row as
[+w, -w] (and [+b, -b]) over the sorted classes, which preserves the
argmax exactly and gives the reader self-consistent probabilities.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import subprocess
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

REPO_ROOT = Path(__file__).resolve().parent.parent

MANIFEST = REPO_ROOT / "dataset" / "manifest.csv"
EXTRACTOR = REPO_ROOT / "target" / "release" / "compid"
FEATURES_OUT = REPO_ROOT / "dataset" / "features" / "train.jsonl"
MODEL_OUT = REPO_ROOT / "model" / "model.json"

LABEL_COLUMNS = [
    "compiler_family",
    "compiler_version",
    "opt_level",
    "arch",
    "os",
    "abi",
    "pie",
    "lto",
    "static",
]

MODEL_FORMAT = "compid-model/v1"
FEATURE_VERSION = "1"


def log(msg: str) -> None:
    print(msg, flush=True)


def round6(x: float) -> float:
    """Round to 6 significant digits; collapse -0.0 so the JSON stays tidy."""
    v = float(f"{x:.6g}")
    return 0.0 if v == 0.0 else v


def ensure_extractor() -> Path:
    if EXTRACTOR.exists():
        return EXTRACTOR
    log(f"release binary not found, building: {EXTRACTOR}")
    subprocess.run(
        ["cargo", "build", "--release"],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return EXTRACTOR


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def extract_corpus(
    manifest_path: Path, extractor: Path, features_out: Path, limit: int | None = None
) -> list[dict]:
    """Extract features for every manifest row whose binary exists and parses.

    Returns one record per successfully extracted row, in manifest order:
    {"build_id", "labels", "features"} with labels taken from the manifest.
    """
    with manifest_path.open(newline="", encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    if limit is not None:
        rows = rows[:limit]
    log(f"manifest: {len(rows)} rows from {manifest_path.relative_to(REPO_ROOT)}")

    records: list[dict] = []
    skipped: Counter[str] = Counter()
    skipped_reasons: dict[str, str] = {}
    sha_mismatches = 0

    for i, row in enumerate(rows, 1):
        path = REPO_ROOT / row["path"]
        if not path.is_file():
            skipped["missing binary"] += 1
            skipped_reasons.setdefault("missing binary", row["path"])
            continue
        if sha256_of(path) != row["sha256"]:
            sha_mismatches += 1
            print(f"warning: sha256 mismatch for {row['path']}", file=sys.stderr)

        proc = subprocess.run(
            [str(extractor), "extract", str(path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if proc.returncode != 0:
            reason = proc.stderr.decode(errors="replace").strip().splitlines()
            reason = reason[-1] if reason else f"exit code {proc.returncode}"
            if "not an ELF file" in reason:
                key = "not an ELF file (extractor is ELF-only; e.g. windows PE)"
            else:
                key = reason.removeprefix("error: ").strip()[:70] or "extraction failed"
            skipped[key] += 1
            skipped_reasons.setdefault(key, row["path"])
            continue
        try:
            extracted = json.loads(proc.stdout)
        except json.JSONDecodeError:
            skipped["unparseable extractor output"] += 1
            skipped_reasons.setdefault("unparseable extractor output", row["path"])
            continue

        records.append(
            {
                "build_id": row["build_id"],
                "labels": {col: row[col] for col in LABEL_COLUMNS},
                "features": extracted["features"],
            }
        )
        if i % 200 == 0:
            log(f"  extracted {i}/{len(rows)} rows...")

    features_out.parent.mkdir(parents=True, exist_ok=True)
    with features_out.open("w", encoding="utf-8") as f:
        for rec in records:
            f.write(json.dumps(rec, separators=(",", ":")) + "\n")

    log(f"extracted features for {len(records)} binaries -> {features_out.relative_to(REPO_ROOT)}")
    if skipped:
        log(f"skipped {sum(skipped.values())} rows:")
        for key, n in sorted(skipped.items(), key=lambda kv: -kv[1]):
            log(f"  {n:4d}  {key} (e.g. {skipped_reasons[key]})")
    if sha_mismatches:
        log(f"warning: {sha_mismatches} binaries did not match their manifest sha256")
    return records


def prune_by_document_frequency(dicts: list[dict], min_df: int) -> list[dict]:
    """Drop features that appear in fewer than min_df binaries (singletons are noise)."""
    df: Counter[str] = Counter()
    for d in dicts:
        df.update(d.keys())
    keep = {name for name, n in df.items() if n >= min_df}
    return [{k: v for k, v in d.items() if k in keep} for d in dicts]


def softmax(scores: np.ndarray) -> np.ndarray:
    z = scores - scores.max(axis=1, keepdims=True)
    e = np.exp(z)
    return e / e.sum(axis=1, keepdims=True)


def topk_accuracy(probs: np.ndarray, y_true_idx: np.ndarray, k: int) -> float:
    order = np.argsort(-probs, axis=1, kind="stable")
    hits = [y_true_idx[i] in order[i, :k] for i in range(len(y_true_idx))]
    return float(np.mean(hits))


def fit_field(field: str, classes: list[str], dicts: list[dict], y: list[str], args) -> dict | None:
    """Train one field's model; return the export payload plus held-out metrics."""
    vec = DictVectorizer(sparse=True)
    x_all = vec.fit_transform(dicts)
    x_all = normalize(x_all, norm="l2", copy=False)

    class_index = {c: i for i, c in enumerate(classes)}
    y_idx_all = np.array([class_index[v] for v in y])

    idx_train, idx_test = train_test_split(
        np.arange(len(y)),
        test_size=args.test_size,
        random_state=args.random_state,
        stratify=y_idx_all,
    )

    clf = LogisticRegression(max_iter=args.max_iter, C=args.C)
    with warnings.catch_warnings():
        warnings.simplefilter("error", ConvergenceWarning)
        try:
            clf.fit(x_all[idx_train], y_idx_all[idx_train])
        except ConvergenceWarning:
            log(f"  {field}: hit max_iter; keeping the fit and continuing")
            clf = LogisticRegression(max_iter=args.max_iter, C=args.C)
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ConvergenceWarning)
                clf.fit(x_all[idx_train], y_idx_all[idx_train])

    # Export with the rounded weights the Rust reader will actually use.
    if len(classes) == 2:
        # scikit-learn stores one weight row for binary problems (sigmoid form):
        # decision_function > 0 means classes[1]. Mirror it into two softmax
        # rows: classes[1] gets (+w, +b), classes[0] gets (-w, -b). The argmax
        # is unchanged, and softmax over the mirrored pair is monotone in the
        # same logit, so the reader reports self-consistent probabilities.
        wr = [round6(v) for v in np.asarray(clf.coef_[0], dtype=np.float64)]
        b = round6(float(clf.intercept_[0]))
        coef = np.array([[round6(-v) for v in wr], wr])
        intercept = np.array([-b if b != 0.0 else 0.0, b])
    else:
        coef = np.array(
            [[round6(v) for v in row] for row in np.asarray(clf.coef_, dtype=np.float64)]
        )
        intercept = np.array([round6(v) for v in np.asarray(clf.intercept_, dtype=np.float64)])

    def evaluate(idx: np.ndarray) -> dict:
        x = x_all[idx]
        scores = np.asarray(x.dot(coef.T)) + intercept
        probs = softmax(scores)
        y_idx = y_idx_all[idx]
        pred = probs.argmax(axis=1)
        top1 = float(np.mean(pred == y_idx))
        macro_f1 = float(
            f1_score(y_idx, pred, labels=list(range(len(classes))), average="macro", zero_division=0)
        )
        top3 = topk_accuracy(probs, y_idx, 3)
        return {"top1": top1, "top3": top3, "macro_f1": macro_f1}

    test_metrics = evaluate(idx_test)
    train_metrics = evaluate(idx_train)

    return {
        "field": field,
        "classes": list(classes),
        "features": list(vec.get_feature_names_out()),
        "intercept": [float(v) for v in intercept],
        "coef": [[float(v) for v in row] for row in coef],
        "n_rows": len(y),
        "n_train": len(idx_train),
        "n_test": len(idx_test),
        "test": test_metrics,
        "train": train_metrics,
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        prog="train.py",
        description="Build the feature corpus, train one classifier per label field, "
        "and write model/model.json (compid-model/v1).",
    )
    ap.add_argument("--manifest", type=Path, default=MANIFEST, help="manifest CSV (default: %(default)s)")
    ap.add_argument("--extractor", type=Path, default=EXTRACTOR, help="compid binary to extract with")
    ap.add_argument("--features-out", type=Path, default=FEATURES_OUT, help="JSONL corpus output path")
    ap.add_argument("--features-in", type=Path, default=None,
                    help="reuse an existing JSONL corpus instead of extracting")
    ap.add_argument("--extract-only", action="store_true",
                    help="stop after writing the feature corpus; do not train")
    ap.add_argument("--model-out", type=Path, default=MODEL_OUT, help="model JSON output path")
    ap.add_argument("--fields", default=",".join(LABEL_COLUMNS),
                    help="comma-separated label fields to train (default: all nine)")
    ap.add_argument("--min-df", type=int, default=150, metavar="N",
                    help="keep only features present in at least N binaries "
                         "(default: %(default)s; tuned on the seed corpus - raising "
                         "it to 150 matches the accuracy of much smaller thresholds "
                         "while keeping model/model.json about one third the size "
                         "of min-df 75)")
    ap.add_argument("--test-size", type=float, default=0.2, help="hold-out fraction (default: %(default)s)")
    ap.add_argument("--random-state", type=int, default=42, help="split seed (default: %(default)s)")
    ap.add_argument("--max-iter", type=int, default=3000, help="LogisticRegression max_iter (default: %(default)s)")
    ap.add_argument("--C", type=float, default=4.0, help="LogisticRegression inverse regularization (default: %(default)s)")
    ap.add_argument("--limit", type=int, default=None, metavar="N",
                    help="use only the first N manifest rows (smoke runs)")
    args = ap.parse_args()

    extractor = args.extractor if args.extractor.is_absolute() else REPO_ROOT / args.extractor
    if extractor == EXTRACTOR:
        extractor = ensure_extractor()
    if not extractor.exists():
        log(f"error: extractor not found at {extractor}")
        return 1

    if args.features_in is not None:
        records = []
        with args.features_in.open(encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    records.append(json.loads(line))
        log(f"loaded {len(records)} feature records from {args.features_in}")
    else:
        records = extract_corpus(args.manifest, extractor, args.features_out, args.limit)
    if args.extract_only:
        return 0
    if len(records) < 10:
        log("error: too few usable rows to train on")
        return 1

    dicts = [rec["features"] for rec in records]
    dicts = prune_by_document_frequency(dicts, args.min_df)
    vocab = Counter(k for d in dicts for k in d)
    log(f"vocabulary: {len(vocab)} features after min-df={args.min_df} pruning")
    by_prefix = Counter(name.split(":", 1)[0] for name in vocab)
    log("  by prefix: " + ", ".join(f"{p}={n}" for p, n in sorted(by_prefix.items())))

    wanted = [f.strip() for f in args.fields.split(",") if f.strip()]
    fields_payload: dict[str, dict] = {}
    report_rows: list[dict] = []

    for field in wanted:
        y = [rec["labels"][field] for rec in records]
        classes = sorted(set(y))
        if len(classes) < 2:
            log(f"field {field}: only {classes} present -> skipped (needs 2+ classes)")
            report_rows.append({"field": field, "classes": classes, "n": len(y), "skipped": True})
            continue
        log(f"training {field}: {len(classes)} classes {classes} over {len(y)} rows")
        payload = fit_field(field, classes, dicts, y, args)
        if payload is None:
            report_rows.append({"field": field, "classes": classes, "n": len(y), "skipped": True})
            continue
        fields_payload[field] = {
            "classes": payload["classes"],
            "intercept": payload["intercept"],
            "coef": payload["coef"],
        }
        t = payload["test"]
        log(
            f"  hold-out: top-1 {t['top1']:.4f}  top-3 {t['top3']:.4f}  macro-F1 {t['macro_f1']:.4f}"
            f"  (train top-1 {payload['train']['top1']:.4f}, {payload['n_test']} held out)"
        )
        report_rows.append({**payload, "n": len(y), "skipped": False})

    manifest_rows = sum(1 for _ in open(MANIFEST, encoding="utf-8")) - 1
    model = {
        "format": MODEL_FORMAT,
        "feature_version": FEATURE_VERSION,
        "created": date.today().isoformat(),
        "features": sorted(vocab),
        "fields": {name: fields_payload[name] for name in sorted(fields_payload)},
        # Provenance: lets a user tell how fresh the model they are running
        # is, and tie it back to the exact dataset it was fitted on. The
        # Rust reader treats the whole block as optional.
        "provenance": {
            "manifest_rows": manifest_rows,
            "manifest_sha256": sha256_of(MANIFEST),
            "features_rows": len(records),
            "features_sha256": sha256_of(args.features_in) if args.features_in else None,
        },
    }
    args.model_out.parent.mkdir(parents=True, exist_ok=True)
    with args.model_out.open("w", encoding="utf-8") as f:
        json.dump(model, f, separators=(",", ":"))
    size_kb = args.model_out.stat().st_size / 1024
    log(f"wrote {args.model_out.relative_to(REPO_ROOT)} ({size_kb:.0f} KiB, "
        f"{len(model['features'])} features, {len(fields_payload)} fields)")

    log("\nevaluation report (hold-out, split seed %d):" % args.random_state)
    log(f"{'field':<18} {'classes':>7} {'rows':>6} {'top-1':>7} {'top-3':>7} {'macro-F1':>9}")
    for row in report_rows:
        if row["skipped"]:
            log(f"{row['field']:<18} {len(row['classes']):>7} {row['n']:>6}   skipped (needs 2+ classes)")
        else:
            t = row["test"]
            log(f"{row['field']:<18} {len(row['classes']):>7} {row['n']:>6} {t['top1']:>7.4f} "
                f"{t['top3']:>7.4f} {t['macro_f1']:>9.4f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
