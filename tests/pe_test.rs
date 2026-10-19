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
