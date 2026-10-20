//! Feature extraction: turn parsed binary material ([`ElfInfo`],
//! [`PeInfo`], or [`MachoInfo`]) into the tf feature map the trainer and the
//! classifier share.
//!
//! Feature keys, exactly as the project contract defines them:
//!
//! | prefix | meaning |
//! |--------|---------|
//! | `ng:<hex>` | byte 3-gram counts over the raw `.text` bytes |
//! | `mn:<name>` | disassembled mnemonic counts |
//! | `rg:<reg>` | register occurrence counts (operands and memory bases) |
//! | `cn:<hex>` | immediate constants masked to 32 bits |
//! | `sec:<s>` | floor(log2(section size)) for each named section |
//! | `ent:<s>` | Shannon-entropy bucket 0..=8 of each section's bytes |
//! | `meta:windows` | number of cleanly disassembling 64-byte windows (ELF and PE) |
//! | `meta:macos` | the same count on Mach-O; the macOS container marker |
//! | `cmt:<tok>` | tokens from the `.comment` section; import-table DLL names on PE; dylib load-command names on Mach-O |
//!
//! Disassembly windows are consecutive non-overlapping 64-byte chunks of
//! `.text`. A window "disassembles cleanly" when decoding from its first
//! byte consumes exactly the window with no decode errors; only clean
//! windows contribute mnemonic, register, and immediate counts, so every
//! instruction is counted at most once. Byte 3-grams run over the raw bytes
//! regardless of decode success, which is what makes the feature work on
//! data sections and on packers that break disassembly.
//!
//! All counts are raw integers; L2 normalization happens at train and infer
//! time, never in the stored JSON.

use crate::elfinfo::{ElfInfo, SectionInfo};
use crate::machoinfo::MachoInfo;
use crate::peinfo::PeInfo;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use yaxpeax_arch::LengthedInstruction;
use yaxpeax_x86::long_mode::{InstDecoder, Operand};

/// Disassembly window size in bytes.
pub const WINDOW_SIZE: usize = 64;

/// The nine provenance labels attached to every feature record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    pub compiler_family: String,
    pub compiler_version: String,
    pub opt_level: String,
    pub arch: String,
    pub os: String,
    pub abi: String,
    pub pie: String,
    pub lto: String,
    #[serde(rename = "static")]
    pub static_linking: String,
}

/// One extracted binary: provenance labels plus its feature vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFeatures {
    pub build_id: String,
    pub labels: Labels,
    pub features: BTreeMap<String, u64>,
}

impl ExtractedFeatures {
    pub fn new(build_id: String, labels: Labels) -> Self {
        ExtractedFeatures {
            build_id,
            labels,
            features: BTreeMap::new(),
        }
    }

    fn bump(&mut self, key: String) {
        *self.features.entry(key).or_insert(0) += 1;
    }
}

impl Labels {
    /// Labels as the ELF extractor reads them (opt_level stays "unknown";
    /// the manifest is ground truth for training).
    pub fn from_elf(info: &ElfInfo) -> Labels {
        let l = &info.labels;
        Labels {
            compiler_family: l.compiler_family.clone(),
            compiler_version: l.compiler_version.clone(),
            opt_level: l.opt_level.clone(),
            arch: l.arch.clone(),
            os: l.os.clone(),
            abi: l.abi.clone(),
            pie: l.pie.clone(),
            lto: l.lto.clone(),
            static_linking: l.static_linking.clone(),
        }
    }

    /// Labels as the PE extractor reads them.
    pub fn from_pe(info: &PeInfo) -> Labels {
        let l = &info.labels;
        Labels {
            compiler_family: l.compiler_family.clone(),
            compiler_version: l.compiler_version.clone(),
            opt_level: l.opt_level.clone(),
            arch: l.arch.clone(),
            os: l.os.clone(),
            abi: l.abi.clone(),
            pie: l.pie.clone(),
            lto: l.lto.clone(),
            static_linking: l.static_linking.clone(),
        }
    }

    /// Labels as the Mach-O extractor reads them.
    pub fn from_macho(info: &MachoInfo) -> Labels {
        let l = &info.labels;
        Labels {
            compiler_family: l.compiler_family.clone(),
            compiler_version: l.compiler_version.clone(),
            opt_level: l.opt_level.clone(),
            arch: l.arch.clone(),
            os: l.os.clone(),
            abi: l.abi.clone(),
            pie: l.pie.clone(),
            lto: l.lto.clone(),
            static_linking: l.static_linking.clone(),
        }
    }
}

/// Borrowed, format-agnostic material one extraction pass consumes. Both
/// [`ElfInfo`] and [`PeInfo`] project onto this, so the loop below is the
/// single place n-grams, windows, geometry, and tokens are counted - which
/// is what keeps the ELF and PE feature schemas byte-for-byte identical.
struct Source<'a> {
    labels: Labels,
    build_id: &'a str,
    text: &'a [u8],
    sections: &'a [SectionInfo],
    section_bytes: &'a dyn Fn(usize, u64) -> Vec<u8>,
    /// Residue tokens counted as `cmt:` features: `.comment` banners on
    /// ELF, import-table DLL names on PE, dylib load-command names on
    /// Mach-O.
    tokens: Vec<String>,
    /// The `meta:` key the clean-window count lands under: `meta:windows`
    /// on ELF and PE, `meta:macos` on Mach-O (the macOS container marker,
    /// mirroring how `meta:windows` doubles as the PE one).
    meta_key: &'static str,
    /// Run x86-64 long-mode disassembly over `text`. False skips the
    /// mnemonic/register/immediate features (and leaves the window count at
    /// 0) for code bytes this decoder cannot read, e.g. an arm64 Mach-O.
    disassemble: bool,
}

/// The shared core: n-grams, windowed disassembly, section geometry and
/// entropy, residue tokens. Infallible; the per-format wrappers own the
/// "is there a .text at all" checks.
fn extract_source(src: Source) -> ExtractedFeatures {
    let mut ef = ExtractedFeatures::new(src.build_id.to_string(), src.labels);

    // Byte 3-grams over the whole .text, one byte at a time.
    for gram in src.text.windows(3) {
        let key = format!("ng:{:02x}{:02x}{:02x}", gram[0], gram[1], gram[2]);
        ef.bump(key);
    }

    // Windowed disassembly.
    let decoder = InstDecoder::default();
    let mut windows = 0u64;
    if src.disassemble {
        for window in src.text.chunks_exact(WINDOW_SIZE) {
            if let Some(stats) = disassemble_window(&decoder, window) {
                windows += 1;
                for m in &stats.mnemonics {
                    ef.bump(format!("mn:{m}"));
                }
                for r in &stats.registers {
                    ef.bump(format!("rg:{r}"));
                }
                for imm in &stats.immediates {
                    ef.bump(format!("cn:{imm:x}"));
                }
            }
        }
    }
    ef.features.insert(src.meta_key.to_string(), windows);

    // Section geometry and byte entropy.
    for s in src.sections {
        if s.size > 0 {
            ef.features
                .insert(format!("sec:{}", s.name), log2_size(s.size));
        }
        if !s.no_bits && s.size > 0 {
            let bytes = (src.section_bytes)(s.offset, s.size);
            ef.features
                .insert(format!("ent:{}", s.name), entropy_bucket(&bytes));
        }
    }

    // Residue tokens: compiler banners, import DLL names.
    for tok in &src.tokens {
        ef.bump(format!("cmt:{tok}"));
    }

    ef
}

/// Extract the full feature map from parsed ELF material.
pub fn extract(info: &ElfInfo) -> Result<ExtractedFeatures> {
    let text = match &info.text {
        Some(t) if !t.is_empty() => t,
        Some(_) => bail!("binary has an empty .text section; nothing to fingerprint"),
        None => bail!(
            "binary has no readable .text section (stripped section headers?); \
             compid needs section headers to find executable bytes"
        ),
    };

    Ok(extract_source(Source {
        labels: Labels::from_elf(info),
        build_id: &info.build_id,
        text,
        sections: &info.sections,
        section_bytes: &|offset, size| info.section_data(offset, size),
        tokens: info
            .comment
            .as_deref()
            .map(comment_tokens)
            .unwrap_or_default(),
        meta_key: "meta:windows",
        disassemble: true,
    }))
}

/// Extract the full feature map from parsed PE material. Import-table DLL
/// names share the `cmt:` namespace - they are the strongest OS/libc signal
/// a Windows binary carries. A non-x86-64 PE (none in the seed today)
/// yields the byte-level features only, with `meta:windows` at 0.
pub fn extract_pe(info: &PeInfo) -> Result<ExtractedFeatures> {
    let text = match &info.text {
        Some(t) if !t.is_empty() => t,
        Some(_) => bail!("binary has an empty .text section; nothing to fingerprint"),
        None => bail!(
            "binary has no readable .text section (section absent or raw size zero); \
             compid needs PE section headers to find executable bytes"
        ),
    };

    Ok(extract_source(Source {
        labels: Labels::from_pe(info),
        build_id: &info.build_id,
        text,
        sections: &info.sections,
        section_bytes: &|offset, size| info.section_data(offset, size),
        tokens: info
            .libraries
            .iter()
            .map(|lib| lib.to_ascii_lowercase())
            .collect(),
        meta_key: "meta:windows",
        disassemble: info.disassembles_x86_64(),
    }))
}

/// Extract the full feature map from parsed Mach-O material. Dylib
/// load-command names share the `cmt:` namespace - they are the strongest
/// OS signal a macOS binary carries. The clean-window count files under
/// `meta:macos`, the container marker mirroring PE's `meta:windows`. A
/// non-x86-64 Mach-O (arm64 today) yields the byte-level features only,
/// with `meta:macos` at 0.
pub fn extract_macho(info: &MachoInfo) -> Result<ExtractedFeatures> {
    let text = match &info.text {
        Some(t) if !t.is_empty() => t,
        Some(_) => bail!("binary has an empty __TEXT,__text section; nothing to fingerprint"),
        None => bail!(
            "binary has no readable __TEXT,__text section (segment load commands absent or \
             the section holds no file bytes); compid needs Mach-O segment load commands \
             to find executable bytes"
        ),
    };

    Ok(extract_source(Source {
        labels: Labels::from_macho(info),
        build_id: &info.build_id,
        text,
        sections: &info.sections,
        section_bytes: &|offset, size| info.section_data(offset, size),
        tokens: info
            .libraries
            .iter()
            .map(|lib| lib.to_ascii_lowercase())
            .collect(),
        meta_key: "meta:macos",
        disassemble: info.disassembles_x86_64(),
    }))
}

/// Mnemonic, register, and immediate counts from one 64-byte window, or
/// `None` if the window does not disassemble cleanly.
#[derive(Debug, Default, Clone)]
pub struct WindowStats {
    pub mnemonics: Vec<String>,
    pub registers: Vec<String>,
    pub immediates: Vec<u32>,
}

pub fn disassemble_window(decoder: &InstDecoder, window: &[u8]) -> Option<WindowStats> {
    let mut stats = WindowStats::default();
    let mut off = 0usize;
    let mut budget = window.len(); // one instruction consumes at least one byte
    while off < window.len() {
        if budget == 0 {
            return None; // pathological: more instructions than bytes
        }
        match decoder.decode_slice(&window[off..]) {
            Ok(inst) => {
                let len = LengthedInstruction::len(&inst).to_const() as usize;
                if len == 0 || len > window.len() - off {
                    return None;
                }
                stats.mnemonics.push(inst.opcode().to_string());
                collect_operands(&inst, &mut stats);
                off += len;
            }
            Err(_) => return None,
        }
        budget -= 1;
    }
    Some(stats)
}

fn collect_operands(inst: &yaxpeax_x86::long_mode::Instruction, stats: &mut WindowStats) {
    for i in 0..inst.operand_count() {
        match inst.operand(i) {
            Operand::ImmediateI8 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateU8 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateI16 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateU16 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateI32 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateU32 { imm } => push_imm(stats, imm as i64),
            Operand::ImmediateI64 { imm } => push_imm(stats, imm),
            Operand::ImmediateU64 { imm } => push_imm(stats, imm as i64),
            other => collect_registers(&other, stats),
        }
    }
}

fn push_imm(stats: &mut WindowStats, value: i64) {
    stats.immediates.push((value & 0xffff_ffff) as u32);
}

/// Register names referenced anywhere in an operand, including memory
/// operand base/index registers and AVX-512 mask registers.
fn collect_registers(op: &Operand, stats: &mut WindowStats) {
    use Operand::*;
    match op {
        Register { reg } => push_reg(stats, reg.name()),
        RegisterMaskMerge { reg, mask, .. } => {
            push_reg(stats, reg.name());
            push_reg(stats, mask.name());
        }
        RegisterMaskMergeSae { reg, mask, .. } => {
            push_reg(stats, reg.name());
            push_reg(stats, mask.name());
        }
        RegisterMaskMergeSaeNoround { reg, mask, .. } => {
            push_reg(stats, reg.name());
            push_reg(stats, mask.name());
        }
        MemDeref { base } => push_reg(stats, base.name()),
        Disp { base, .. } => push_reg(stats, base.name()),
        MemIndexScale { index, .. } => push_reg(stats, index.name()),
        MemIndexScaleDisp { index, .. } => push_reg(stats, index.name()),
        MemBaseIndexScale { base, index, .. } => {
            push_reg(stats, base.name());
            push_reg(stats, index.name());
        }
        MemBaseIndexScaleDisp { base, index, .. } => {
            push_reg(stats, base.name());
            push_reg(stats, index.name());
        }
        MemDerefMasked { base, mask } => {
            push_reg(stats, base.name());
            push_reg(stats, mask.name());
        }
        DispMasked { base, mask, .. } => {
            push_reg(stats, base.name());
            push_reg(stats, mask.name());
        }
        MemIndexScaleMasked { index, mask, .. } => {
            push_reg(stats, index.name());
            push_reg(stats, mask.name());
        }
        MemIndexScaleDispMasked { index, mask, .. } => {
            push_reg(stats, index.name());
            push_reg(stats, mask.name());
        }
        MemBaseIndexScaleMasked {
            base, index, mask, ..
        } => {
            push_reg(stats, base.name());
            push_reg(stats, index.name());
            push_reg(stats, mask.name());
        }
        MemBaseIndexScaleDispMasked {
            base, index, mask, ..
        } => {
            push_reg(stats, base.name());
            push_reg(stats, index.name());
            push_reg(stats, mask.name());
        }
        _ => {}
    }
}

fn push_reg(stats: &mut WindowStats, name: &'static str) {
    stats.registers.push(name.to_string());
}

/// floor(log2(size)); 0 maps to 0.
pub fn log2_size(size: u64) -> u64 {
    if size <= 1 {
        0
    } else {
        (size as f64).log2().floor() as u64
    }
}

/// Shannon entropy of a byte sequence in bits/byte (0.0 ..= 8.0).
pub fn shannon_entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut hist = [0u64; 256];
    for &b in bytes {
        hist[b as usize] += 1;
    }
    let n = bytes.len() as f64;
    hist.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// floor(shannon_entropy) clamped to 0..=8.
pub fn entropy_bucket(bytes: &[u8]) -> u64 {
    let e = shannon_entropy(bytes);
    (e.floor().clamp(0.0, 8.0)) as u64
}

/// Lowercase tokens of length >= 3 from a `.comment` blob (NUL/whitespace
/// separated). Compilers leave banners here that survive stripping.
pub fn comment_tokens(comment: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(comment);
    s.split(['\0', '\n', '\r', '\t', ' '])
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Cosine similarity between two feature vectors. Two empty (or one empty)
/// vectors have similarity 0.0 by definition here.
pub fn cosine(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> f64 {
    let norm_a = a.values().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b = b.values().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    let dot = a
        .iter()
        .filter_map(|(k, va)| b.get(k).map(|vb| (*va as f64) * (*vb as f64)))
        .sum::<f64>();
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}
