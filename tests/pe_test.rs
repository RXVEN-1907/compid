//! Extraction and classification tests over the real Windows PE binaries
//! the corpus builder produced (`dataset/binaries/*windows-gnu*`). Those
//! binaries are local build artifacts and are not committed, so the tests
//! report and skip when none are present (fresh checkouts, CI); the
//! synthetic-PE tests in `synthetic_pe_test.rs` keep the extractor covered
//! either way.

use std::path::{Path, PathBuf};
use std::process::Command;

fn windows_binaries() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("dataset/binaries");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains("windows-gnu") {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn pe_binaries_extract_with_windows_labels() {
    let bins = windows_binaries();
    if bins.is_empty() {
        eprintln!("skipping: no dataset/binaries/*windows-gnu* binaries present");
        return;
    }

    for path in &bins {
        let info = peinfo_load(path);
        assert_eq!(info.labels.os, "windows", "{} is windows", path.display());
        assert_eq!(info.labels.arch, "x86_64", "{} is x86_64", path.display());
        // The manifest's windows-row conventions, mirrored by the extractor.
        assert_eq!(info.labels.pie, "n/a", "{} pie is n/a", path.display());
        assert_eq!(
            info.labels.static_linking,
            "n/a",
            "{} static is n/a",
            path.display()
        );
        assert_eq!(info.labels.lto, "off", "{} lto is off", path.display());

        let ef = compid::features::extract_pe(&info)
            .unwrap_or_else(|e| panic!("{} extracts: {e}", path.display()));
        assert!(!ef.features.is_empty(), "{} has features", path.display());
        assert!(
            ef.features.keys().any(|k| k.starts_with("ng:")),
            "{} has byte n-grams",
            path.display()
        );
        assert!(
            ef.features.keys().any(|k| k.starts_with("cmt:")),
            "{} counts its import DLLs as cmt: features",
            path.display()
        );
        assert!(
            ef.features.get("meta:windows").copied().unwrap_or(0) > 0,
            "{} disassembles at least one clean window",
            path.display()
        );
        assert!(
            ef.features.contains_key("sec:.text") && ef.features.contains_key("ent:.text"),
            "{} carries .text geometry and entropy",
            path.display()
        );

        // Import DLL names are real OS signals: every seed windows binary
        // talks to KERNEL32.
        assert!(
            ef.features.contains_key("cmt:kernel32.dll"),
            "{} imports kernel32.dll",
            path.display()
        );
    }
}

#[test]
fn pe_classify_runs_end_to_end_with_the_shipped_model() {
    let bins = windows_binaries();
    if bins.is_empty() {
        eprintln!("skipping: no dataset/binaries/*windows-gnu* binaries present");
        return;
    }
    let model = Path::new(env!("CARGO_MANIFEST_DIR")).join("model/model.json");
    if !model.exists() {
        eprintln!("skipping: no trained model at {}", model.display());
        return;
    }

    let path = &bins[0];
    let out = Command::new(env!("CARGO_BIN_EXE_compid"))
        .args([
            "classify",
            path.to_str().expect("utf-8 path"),
            "--json",
            "--model",
            model.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("classify runs");
    assert!(
        out.status.success(),
        "classify exits 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("classify --json prints JSON");
    let fields = report
        .get("fields")
        .and_then(|f| f.as_array())
        .expect("fields array")
        .clone();
    assert!(!fields.is_empty(), "the model reports predictions");

    // The retrained model ships an os field (linux/windows) learned from the
    // full manifest; every PE binary in the corpus is windows-ground-truth.
    let os = fields
        .iter()
        .find(|f| f.get("field").and_then(|x| x.as_str()) == Some("os"))
        .expect("the shipped model carries an os field");
    let predicted = os.get("predicted").and_then(|x| x.as_str());
    assert_eq!(
        predicted,
        Some("windows"),
        "PE binary classifies as windows"
    );
}

fn peinfo_load(path: &Path) -> compid::peinfo::PeInfo {
    compid::peinfo::load(path).unwrap_or_else(|e| panic!("{} parses as PE: {e}", path.display()))
}

/// The committed DLL fixture carries a real export directory: the four
/// named exports of `corpus/sources/export_demo.c` enumerate as functions
/// with delta-derived sizes, raw names, and the standard extraction
/// wiring. Built by `corpus/build_fixtures.py` (the corpus's own DLL rows
/// live in the gitignored dataset/binaries).
#[test]
fn pe_dll_fixture_enumerates_exported_functions() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/zig-cc_O2_x86_64_dll_sample");
    if !path.exists() {
        eprintln!("skipping: {path:?} not present; run corpus/build_fixtures.py");
        return;
    }
    let info = peinfo_load(&path);
    assert!(
        info.has_exports,
        "{} carries an export directory",
        path.display()
    );
    let names: Vec<&str> = info.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["export_add3", "export_djb2", "export_mix", "export_scale"],
        "the four named exports, ordered by RVA: {names:?}"
    );
    // Derived sizes: three small next-export deltas, then the last
    // export's section-end estimate (the documented heuristic).
    assert_eq!(info.functions[0].addr, 0x1000);
    for f in &info.functions[1..4] {
        assert!(f.size > 0, "{} carries a derived size", f.name);
    }
    assert!(
        info.functions[3].size > info.functions[0].size,
        "the last export's section-end estimate dwarfs a function delta"
    );

    let ex = compid::functions::extract_pe(&info);
    assert_eq!(
        ex.source,
        compid::functions::FunctionSource::SymbolTable,
        "export-table boundaries are a symbol source"
    );
    assert!(
        ex.reason.is_none(),
        "no reason for an exporting DLL: {:?}",
        ex.reason
    );
    assert_eq!(ex.functions.len(), 4);
    for f in &ex.functions {
        assert!(!f.features.is_empty(), "{} has features", f.name);
        assert!(
            f.features.keys().any(|k| k.starts_with("fn:")),
            "{} carries an fn: bucket",
            f.name
        );
        assert!(
            f.features.keys().any(|k| k.starts_with("ng:")),
            "{} carries byte n-grams",
            f.name
        );
        assert!(
            !f.features.keys().any(|k| k.starts_with("cmt:")),
            "{} carries no container residue",
            f.name
        );
    }

    // Classify-compatible: the shipped per-function model predicts on each
    // feature vector without error.
    let model_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("model/functions.json");
    if model_path.exists() {
        let m = compid::model::Model::load(&model_path).expect("functions model loads");
        for f in &ex.functions {
            let pred = m.predict(&f.features);
            assert!(
                pred.fields
                    .iter()
                    .any(|field| field.field == "compiler_family"),
                "{} predicts a compiler_family",
                f.name
            );
        }
    }
}

/// A plain PE executable with no export directory falls back to the
/// stripped-binary heuristic (x86-64): the boundaries come from the
/// prologue/call-target scan over the section table's executable sections
/// with synthesized `func_<rva>` names, or, when that scan finds nothing,
/// an explicit reason. Uses the corpus windows binaries when present;
/// reports and skips otherwise.
#[test]
fn pe_without_exports_falls_back_to_the_heuristic() {
    let bins = windows_binaries();
    if bins.is_empty() {
        eprintln!("skipping: no dataset/binaries/*windows-gnu* binaries present");
        return;
    }
    let plain = match bins.iter().find(|path| !peinfo_load(path).has_exports) {
        Some(path) => path,
        None => {
            eprintln!("skipping: every present windows binary carries exports");
            return;
        }
    };
    let info = peinfo_load(plain);
    assert!(info.functions.is_empty());
    assert_eq!(
        info.labels.arch, "x86_64",
        "the corpus is x86_64; a non-x86-64 PE keeps the export requirement"
    );
    let ex = compid::functions::extract_pe(&info);
    assert_eq!(
        ex.source,
        compid::functions::FunctionSource::Heuristic,
        "a plain x86-64 PE's boundaries come from the heuristic"
    );
    if ex.functions.is_empty() {
        let reason = ex.reason.expect("reason present");
        assert!(
            reason.contains("no export table") && reason.contains("heuristic found no candidate"),
            "reason: {reason}"
        );
    } else {
        assert!(ex.reason.is_none(), "reason: {:?}", ex.reason);
        for f in &ex.functions {
            assert!(f.name.starts_with("func_"), "synthesized name: {}", f.name);
            assert!(f.size >= 16, "minimum size holds: {}", f.name);
            assert!(
                f.features.keys().any(|k| k.starts_with("fn:")),
                "{} carries an fn: bucket",
                f.name
            );
            assert!(
                !f.features.keys().any(|k| k.starts_with("cmt:")),
                "{} carries no container residue",
                f.name
            );
        }
        for pair in ex.functions.windows(2) {
            assert!(pair[0].addr < pair[1].addr, "sorted by address");
        }
    }
}

/// Report what the binaries directory holds right now.
#[test]
fn pe_corpus_presence_is_reported() {
    let n = windows_binaries().len();
    eprintln!(
        "dataset/binaries at {}: {n} windows-gnu PE binary(ies)",
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("dataset/binaries")
            .display()
    );
}
