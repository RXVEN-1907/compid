//! Extraction tests over the committed tiny fixtures in `tests/fixtures/`.
//!
//! Fixture filenames encode their ground truth:
//! `<family><ver>_O<level>_x86_64_<details>`, e.g.
//! `gcc13_O2_x86_64_pie_dynamic`. When no fixtures matching
//! `gcc*_O*_x86_64_*` are present (yet), these tests report and skip rather
//! than fail - the synthetic-ELF tests in `synthetic_elf_test.rs` keep the
//! extractor covered either way.

use compid::{elfinfo, features};
use std::path::{Path, PathBuf};

fn fixtures_matching() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("gcc") && name.contains("_O") && name.contains("_x86_64_") {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn fixture_features_extract_and_stay_in_schema() {
    let fixtures = fixtures_matching();
    if fixtures.is_empty() {
        eprintln!("skipping: no fixtures matching gcc*_O*_x86_64_* in tests/fixtures yet");
        return;
    }

    for path in &fixtures {
        let info =
            elfinfo::load(path).unwrap_or_else(|e| panic!("{} parses as ELF: {e}", path.display()));
        let ef = features::extract(&info)
            .unwrap_or_else(|e| panic!("{} yields features: {e}", path.display()));

        assert!(!ef.features.is_empty(), "{} has features", path.display());
        assert!(
            ef.features.contains_key("meta:windows"),
            "{} has window count",
            path.display()
        );
        assert!(
            ef.features.keys().any(|k| k.starts_with("ng:")),
            "{} has byte n-grams",
            path.display()
        );
        assert_eq!(ef.labels.arch, "x86-64", "{} is x86-64", path.display());

        // The JSON carries exactly the contract top-level schema, in
        // contract order: build_id, labels, features. (Checked on the raw
        // string: re-parsing into a serde_json::Value sorts map keys.)
        let json = serde_json::to_string(&ef).expect("serializes");
        assert!(
            json.starts_with("{\"build_id\":"),
            "{} starts with build_id",
            path.display()
        );
        let labels_pos = json.find("\"labels\":").expect("labels present");
        let features_pos = json.find("\"features\":").expect("features present");
        assert!(
            labels_pos < features_pos,
            "{} emits labels before features",
            path.display()
        );
        let back: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let labels = back
            .get("labels")
            .and_then(|l| l.as_object())
            .expect("labels object");
        for key in [
            "compiler_family",
            "compiler_version",
            "opt_level",
            "arch",
            "os",
            "abi",
            "pie",
            "lto",
            "static",
        ] {
            assert!(
                labels.contains_key(key),
                "{} labels carry {key}",
                path.display()
            );
        }
    }
}

#[test]
fn distinct_fixtures_have_distinct_vectors() {
    let fixtures = fixtures_matching();
    if fixtures.len() < 2 {
        eprintln!("skipping: fewer than two fixtures present");
        return;
    }
    let extracted: Vec<(String, features::ExtractedFeatures)> = fixtures
        .iter()
        .map(|p| {
            let info = elfinfo::load(p).expect("parses");
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                features::extract(&info).expect("extracts"),
            )
        })
        .collect();

    for (i, (name_a, fa)) in extracted.iter().enumerate() {
        for (name_b, fb) in extracted.iter().skip(i + 1) {
            let sim = features::cosine(&fa.features, &fb.features);
            // Different builds of even the same trivial program differ
            // somewhere; identical vectors would mean the extractor is
            // collapsing everything into the same bucket.
            assert!(
                sim < 1.0,
                "{name_a} and {name_b} have identical feature vectors"
            );
        }
    }
}

/// Report what the fixture directory holds right now.
#[test]
fn fixture_dir_presence_is_reported() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let n = fixtures_matching().len();
    eprintln!(
        "tests/fixtures at {}: {n} fixture(s) matching the gcc*_O*_x86_64_* pattern",
        dir.display()
    );
}
