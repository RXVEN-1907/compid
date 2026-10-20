//! compid - binary provenance classifier.
//!
//! Given a (possibly stripped) ELF, PE (x86-64), or Mach-O (x86-64) binary,
//! compid extracts build fingerprints (instruction mix, register mix,
//! immediate constants, byte n-grams, section geometry, comment residue,
//! container names) and predicts compiler family, optimization level,
//! architecture, OS, libc, PIE, LTO, and linkage with confidence scores.
//! Predictions come from a small linear model (`model/model.json`) that the
//! Python trainer in `training/` produces from a self-generated
//! build-fingerprint corpus. Human-facing output renders through
//! [`report`], colored by the small ANSI engine in [`term`] when the
//! terminal allows it.

pub mod elfinfo;
pub mod features;
pub mod functions;
pub mod machoinfo;
pub mod manifest;
pub mod model;
pub mod peinfo;
pub mod report;
pub mod term;

use anyhow::{bail, Context, Result};
use goblin::mach::header as mh;
use std::path::Path;

/// Read a binary from disk and extract its features in one step. The
/// container format is detected from the leading magic bytes: `\x7fELF`
/// parses as ELF, `MZ` parses as PE, and the Mach-O magics parse as Mach-O
/// (64-bit images only; 32-bit Mach-O is rejected with a clear error);
/// anything else is an error.
pub fn extract_binary(path: &Path) -> Result<features::ExtractedFeatures> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let data = std::sync::Arc::new(data);
    if data.len() >= 4 && &data[0..4] == b"\x7fELF" {
        features::extract(&elfinfo::parse(std::sync::Arc::clone(&data))?)
    } else if data.len() >= 2 && &data[0..2] == b"MZ" {
        features::extract_pe(&peinfo::parse(data)?)
    } else if data.len() >= 4 {
        // Read as little-endian so a standard (little-endian) Mach-O magic
        // lands on MH_MAGIC_64/MH_MAGIC and a byteswapped one on the
        // MH_CIGAM values.
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        match magic {
            mh::MH_MAGIC_64 | mh::MH_CIGAM_64 | mh::MH_MAGIC | mh::MH_CIGAM => {
                features::extract_macho(&machoinfo::parse(data)?)
            }
            _ => bail!(
                "{}: unsupported binary format (missing \\x7fELF, MZ, or Mach-O magic); \
                 compid reads ELF, PE, and Mach-O",
                path.display()
            ),
        }
    } else {
        bail!(
            "{}: unsupported binary format (missing \\x7fELF, MZ, or Mach-O magic); \
             compid reads ELF, PE, and Mach-O",
            path.display()
        );
    }
}

/// Read a binary from disk and extract per-function features in one step.
/// Same container detection as [`extract_binary`]. Function boundaries come
/// from the ELF symbol table (unstripped ELF, and relocatable objects) or,
/// for a stripped x86-64 ELF, from the conservative prologue heuristic in
/// [`functions`] (marked `source: Heuristic` in the result). PE and Mach-O
/// yield an empty function list with a reason.
pub fn extract_binary_functions(path: &Path) -> Result<functions::FunctionsExtraction> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let data = std::sync::Arc::new(data);
    if data.len() >= 4 && &data[0..4] == b"\x7fELF" {
        Ok(functions::extract_elf(&elfinfo::parse(
            std::sync::Arc::clone(&data),
        )?))
    } else if data.len() >= 2 && &data[0..2] == b"MZ" {
        Ok(functions::extract_pe(&peinfo::parse(data)?))
    } else if data.len() >= 4 {
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        match magic {
            mh::MH_MAGIC_64 | mh::MH_CIGAM_64 | mh::MH_MAGIC | mh::MH_CIGAM => {
                Ok(functions::extract_macho(&machoinfo::parse(data)?))
            }
            _ => bail!(
                "{}: unsupported binary format (missing \\x7fELF, MZ, or Mach-O magic); \
                 compid reads ELF, PE, and Mach-O",
                path.display()
            ),
        }
    } else {
        bail!(
            "{}: unsupported binary format (missing \\x7fELF, MZ, or Mach-O magic); \
             compid reads ELF, PE, and Mach-O",
            path.display()
        );
    }
}

/// Measure the stripped-binary prologue heuristic against symbol-table
/// ground truth for one ELF on disk. The inverse of the functions path's
/// usual direction: pass an unstripped binary, get precision/recall of the
/// heuristic's starts against the symbol table's starts (see
/// [`functions::measure_boundaries`]). Errors for a non-ELF file.
pub fn measure_binary_functions(path: &Path) -> Result<functions::BoundaryMetrics> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if data.len() < 4 || &data[0..4] != b"\x7fELF" {
        bail!(
            "{}: boundary measurement needs an ELF binary (\\x7fELF magic missing)",
            path.display()
        );
    }
    functions::measure_boundaries(&elfinfo::parse(std::sync::Arc::new(data))?)
}
