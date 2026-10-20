//! Extraction and classification tests over the committed Mach-O fixtures
//! (`tests/fixtures/macho_x86_64_sample` and
//! `tests/fixtures/macho_aarch64_sample`, real zig-built executables).

use std::path::{Path, PathBuf};
use std::process::Command;

fn macho_fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("macho_") {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn macho_fixtures_extract_with_macos_labels() {
    for path in macho_fixtures() {
        let info = macho_load(&path);

        // The container is macOS by construction; arch follows the header
        // cputype, and the manifest-convention axes the container does not
        // express stay n/a.
        assert_eq!(info.labels.os, "macos", "{} is macos", path.display());
        assert_eq!(info.labels.pie, "pie", "{} is PIE", path.display());
        assert_eq!(info.labels.abi, "n/a", "{} abi is n/a", path.display());
        assert_eq!(
            info.labels.static_linking,
            "n/a",
            "{} static is n/a",
            path.display()
        );
        if path.to_string_lossy().contains("x86_64") {
            assert_eq!(info.labels.arch, "x86_64", "{} is x86_64", path.display());
            assert!(
                info.disassembles_x86_64(),
                "{} feeds the x86-64 decoder",
                path.display()
            );
        } else {
            assert_eq!(info.labels.arch, "aarch64", "{} is aarch64", path.display());
            assert!(
                !info.disassembles_x86_64(),
                "{} skips the x86-64 decoder",
                path.display()
            );
        }

        // The zig builds carry an LC_UUID load command.
        assert_ne!(
            info.build_id,
            "none",
            "{} carries an LC_UUID",
            path.display()
        );

        let ef = compid::features::extract_macho(&info)
            .unwrap_or_else(|e| panic!("{} extracts: {e}", path.display()));
        assert!(!ef.features.is_empty(), "{} has features", path.display());
        assert!(
            ef.features.keys().any(|k| k.starts_with("ng:")),
            "{} has byte n-grams over __text",
            path.display()
        );
        assert!(
            ef.features.contains_key("sec:__TEXT,__text")
                && ef.features.contains_key("ent:__TEXT,__text"),
            "{} carries __text geometry and entropy",
            path.display()
        );
        assert!(
            ef.features.contains_key("meta:macos"),
            "{} carries the macOS container marker",
            path.display()
        );
        // The zig builds link libSystem; its install name arrives as a
        // lowercase basename token.
        assert!(
            ef.features.contains_key("cmt:libsystem.b.dylib"),
            "{} counts its dylib as a cmt: feature",
            path.display()
        );

        // Disassembly runs only where the decoder can read the bytes and
        // only over complete windows: whenever the window count is zero
        // (the x86_64 sample's 20-byte __text cannot fill a 64-byte window;
        // the arm64 sample never disassembles) no mnemonic/register/
        // immediate features may appear.
        let text_len = info.text.as_ref().map(Vec::len).unwrap_or(0);
        assert_eq!(
            ef.features.get("meta:macos").copied().unwrap_or(0),
            (text_len / 64) as u64,
            "{} window count matches its __text extent",
            path.display()
        );
        if ef.features.get("meta:macos").copied().unwrap_or(0) == 0 {
            assert!(
                ef.features.keys().all(|k| !k.starts_with("mn:")),
                "{} ({}-byte __text) yields no mnemonic features",
                path.display(),
                text_len
            );
        }
    }
}

#[test]
fn macho_classify_runs_end_to_end_with_the_shipped_model() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/macho_x86_64_sample")
        .to_path_buf();
    let model = Path::new(env!("CARGO_MANIFEST_DIR")).join("model/model.json");
    if !model.exists() {
        eprintln!("skipping: no trained model at {}", model.display());
        return;
    }

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

    // The retrained model ships an os field (linux/windows/macos) learned
    // from the full manifest; every Mach-O fixture is macos-ground-truth.
    let os = fields
        .iter()
        .find(|f| f.get("field").and_then(|x| x.as_str()) == Some("os"))
        .expect("the shipped model carries an os field");
    let predicted = os.get("predicted").and_then(|x| x.as_str());
    assert_eq!(
        predicted,
        Some("macos"),
        "Mach-O binary classifies as macos"
    );
}

/// Report what the fixture directory holds right now.
#[test]
fn macho_fixture_presence_is_reported() {
    let n = macho_fixtures().len();
    eprintln!(
        "tests/fixtures at {}: {n} Mach-O fixture(s)",
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .display()
    );
}

fn macho_load(path: &Path) -> compid::machoinfo::MachoInfo {
    compid::machoinfo::load(path)
        .unwrap_or_else(|e| panic!("{} parses as Mach-O: {e}", path.display()))
}
