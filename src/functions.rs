//! Function-level provenance: same feature namespaces, scoped to one
//! function's byte range.
//!
//! A function's feature vector holds exactly the byte- and code-shape
//! families the whole-binary extractor uses - `ng:`, `mn:`, `rg:`, `cn:`
//! over the function's own bytes - plus one size indicator `fn:<log2size>`
//! (floor of log2 of the byte length, clamped to 0..=24) and the container
//! marker `meta:windows` (clean 64-byte windows within the function). No
//! `sec:`/`ent:` geometry and no `cmt:` residue: those describe the
//! container, not a function.
//!
//! Function boundaries come from the ELF symbol table this round:
//! SHT_SYMTAB entries of type STT_FUNC with a nonzero `st_size`,
//! deduplicated by `(st_value, st_size)`, raw (never demangled) names.
//! That makes unstripped ELF (and relocatable objects) first-class; PE and
//! Mach-O symbol sources, and heuristics for stripped binaries, remain
//! future work. A stripped ELF, a PE, or a Mach-O yields an empty function
//! list with a reason string instead of guessing boundaries.

use crate::elfinfo::ElfInfo;
use crate::features::{disassemble_window, log2_size, Labels, WINDOW_SIZE};
use crate::machoinfo::MachoInfo;
use crate::peinfo::PeInfo;
use serde::Serialize;
use std::collections::BTreeMap;
use yaxpeax_x86::long_mode::InstDecoder;

/// Upper clamp for the `fn:` size bucket (floor of log2 of the byte length).
pub const FN_SIZE_BUCKET_MAX: u64 = 24;

/// One function's scoped feature vector, ready for the table, the training
/// JSONL, or the model.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionFeatures {
    /// Raw symbol-table name, never demangled.
    pub name: String,
    /// `st_value` as recorded: a virtual address in a linked binary, an
    /// offset within the section in a relocatable object.
    pub addr: u64,
    /// `st_size` in bytes.
    pub size: u64,
    pub features: BTreeMap<String, u64>,
}

/// Per-function extraction result for one binary. `functions` is empty
/// exactly when `reason` is set.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionsExtraction {
    pub build_id: String,
    /// The extractor's own best-effort labels for the container (what
    /// `compid extract` would report); batch mode replaces these with the
    /// manifest's ground-truth labels.
    pub labels: Labels,
    /// One entry per function, ordered by address.
    pub functions: Vec<FunctionFeatures>,
    /// Set when function-level extraction does not apply: a stripped ELF,
    /// or a container whose symbol source is not implemented yet.
    pub reason: Option<String>,
}

/// One training-record row of the functions JSONL: `{"build_id", "name",
/// "addr", "size", "labels", "features"}`, the per-function counterpart of
/// the whole-binary feature record. `addr` is preformatted hexadecimal.
#[derive(Debug, Serialize)]
pub struct FunctionTrainingRow<'a> {
    pub build_id: &'a str,
    pub name: &'a str,
    pub addr: String,
    pub size: u64,
    pub labels: &'a Labels,
    pub features: &'a BTreeMap<String, u64>,
}

/// The `fn:` feature key for a function of `len` bytes: `fn:<floor(log2)>`,
/// clamped to 0..=24.
pub fn size_bucket_key(len: usize) -> String {
    format!("fn:{}", log2_size(len as u64).min(FN_SIZE_BUCKET_MAX))
}

/// Feature counts over one function's bytes: byte 3-grams always; mnemonic,
/// register, and immediate counts from clean 64-byte windows; the clean
/// window count under `meta:windows`; and the size bucket `fn:<log2size>`
/// with value 1. No `sec:`, `ent:`, or `cmt:` keys, ever.
///
/// Like the whole-binary path, disassembly uses x86-64 long mode; functions
/// from other ISAs simply contribute no `mn:`/`rg:`/`cn:` counts and a
/// `meta:windows` of 0, while their byte 3-grams still flow.
pub fn function_features(bytes: &[u8]) -> BTreeMap<String, u64> {
    let mut f: BTreeMap<String, u64> = BTreeMap::new();
    for gram in bytes.windows(3) {
        *f.entry(format!("ng:{:02x}{:02x}{:02x}", gram[0], gram[1], gram[2]))
            .or_insert(0) += 1;
    }

    let decoder = InstDecoder::default();
    let mut clean_windows = 0u64;
    for window in bytes.chunks_exact(WINDOW_SIZE) {
        if let Some(stats) = disassemble_window(&decoder, window) {
            clean_windows += 1;
            for m in &stats.mnemonics {
                *f.entry(format!("mn:{m}")).or_insert(0) += 1;
            }
            for r in &stats.registers {
                *f.entry(format!("rg:{r}")).or_insert(0) += 1;
            }
            for imm in &stats.immediates {
                *f.entry(format!("cn:{imm:x}")).or_insert(0) += 1;
            }
        }
    }
    f.insert("meta:windows".to_string(), clean_windows);

    f.insert(size_bucket_key(bytes.len()), 1);
    f
}

/// Per-function extraction from parsed ELF material. A stripped ELF (or one
/// whose symtab holds no usable function symbols) yields an empty list with
/// a reason; every listed function's bytes are read through
/// [`ElfInfo::file_slice`].
pub fn extract_elf(info: &ElfInfo) -> FunctionsExtraction {
    let labels = Labels::from_elf(info);
    let functions = if info.functions.is_empty() {
        Vec::new()
    } else {
        info.functions
            .iter()
            .map(|fs| FunctionFeatures {
                name: fs.name.clone(),
                addr: fs.addr,
                size: fs.size,
                features: function_features(&info.file_slice(fs.file_offset, fs.size)),
            })
            .collect()
    };
    let reason = if functions.is_empty() {
        Some(
            if info.has_symtab {
                "symbol table carries no usable function symbols (STT_FUNC with \
                 a size in an executable section)"
            } else {
                "no symbol table: binary looks stripped; function extraction \
                 needs an unstripped ELF"
            }
            .to_string(),
        )
    } else {
        None
    };
    FunctionsExtraction {
        build_id: info.build_id.clone(),
        labels,
        functions,
        reason,
    }
}

/// Placeholder for PE: COFF symbol-table support is not wired yet, so the
/// list is empty with an explicit reason.
pub fn extract_pe(info: &PeInfo) -> FunctionsExtraction {
    FunctionsExtraction {
        build_id: info.build_id.clone(),
        labels: Labels::from_pe(info),
        functions: Vec::new(),
        reason: Some(
            "PE function extraction is not implemented yet; function \
             boundaries come from ELF symbol tables only this round"
                .to_string(),
        ),
    }
}

/// Placeholder for Mach-O: same situation as PE.
pub fn extract_macho(info: &MachoInfo) -> FunctionsExtraction {
    FunctionsExtraction {
        build_id: info.build_id.clone(),
        labels: Labels::from_macho(info),
        functions: Vec::new(),
        reason: Some(
            "Mach-O function extraction is not implemented yet; function \
             boundaries come from ELF symbol tables only this round"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_features_hold_only_scoped_namespaces() {
        // 64 bytes of plausible code: push rbp; mov rbp, rsp; mov eax, 0x2a;
        // pop rbp; ret; nop padding.
        let mut bytes = vec![
            0x55, 0x48, 0x89, 0xe5, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0x5d, 0xc3,
        ];
        bytes.resize(64, 0x90);
        let f = function_features(&bytes);

        assert_eq!(f.get("fn:6"), Some(&1), "log2(64) = 6");
        assert_eq!(f.get("meta:windows"), Some(&1));
        assert_eq!(f.get("mn:push"), Some(&1));
        assert_eq!(f.get("cn:2a"), Some(&1));
        // 64 bytes -> 62 3-grams.
        let total: u64 = f
            .iter()
            .filter(|(k, _)| k.starts_with("ng:"))
            .map(|(_, v)| v)
            .sum();
        assert_eq!(total, 62);
        // No container-only namespaces leak into a function's features.
        assert!(!f.keys().any(|k| k.starts_with("sec:")));
        assert!(!f.keys().any(|k| k.starts_with("ent:")));
        assert!(!f.keys().any(|k| k.starts_with("cmt:")));
    }

    #[test]
    fn size_bucket_floors_and_clamps() {
        assert_eq!(size_bucket_key(0), "fn:0");
        assert_eq!(size_bucket_key(1), "fn:0");
        assert_eq!(size_bucket_key(64), "fn:6");
        assert_eq!(size_bucket_key(4096), "fn:12");
        assert_eq!(size_bucket_key(1 << 24), "fn:24");
        assert_eq!(size_bucket_key(1 << 30), "fn:24", "clamped at 24");
    }

    #[test]
    fn short_functions_still_carry_the_size_bucket() {
        // ret alone: 1 byte, no 3-grams, no clean window, but fn:0 present.
        let f = function_features(&[0xc3]);
        assert!(!f.contains_key("ng:c3"), "no 3-gram fits in one byte");
        assert_eq!(f.get("meta:windows"), Some(&0));
        assert_eq!(f.get("fn:0"), Some(&1));
    }
}
