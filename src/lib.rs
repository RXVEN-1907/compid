//! compid - binary provenance classifier.
//!
//! Given a (possibly stripped) ELF or PE (x86-64) binary, compid extracts
//! build fingerprints (instruction mix, register mix, immediate constants,
//! byte n-grams, section geometry, comment residue, import-table names) and
//! predicts compiler family, optimization level, architecture, OS, libc,
//! PIE, LTO, and linkage with confidence scores. Predictions come from a
//! small linear model (`model/model.json`) that the Python trainer in
//! `training/` produces from a self-generated build-fingerprint corpus.

pub mod elfinfo;
pub mod features;
pub mod manifest;
pub mod model;
pub mod peinfo;
pub mod report;

use anyhow::{bail, Context, Result};
use std::path::Path;

/// Read a binary from disk and extract its features in one step. The
/// container format is detected from the leading magic bytes: `\x7fELF`
/// parses as ELF, `MZ` parses as PE; anything else is an error.
pub fn extract_binary(path: &Path) -> Result<features::ExtractedFeatures> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let data = std::sync::Arc::new(data);
    if data.len() >= 4 && &data[0..4] == b"\x7fELF" {
        features::extract(&elfinfo::parse(std::sync::Arc::clone(&data))?)
    } else if data.len() >= 2 && &data[0..2] == b"MZ" {
        features::extract_pe(&peinfo::parse(data)?)
    } else {
        bail!(
            "{}: unsupported binary format (missing \\x7fELF or MZ magic); \
             compid reads ELF and PE",
            path.display()
        );
    }
}
