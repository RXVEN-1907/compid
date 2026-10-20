//! Mach-O (macOS) parsing: pull out the raw material feature extraction
//! needs.
//!
//! Everything here is best-effort provenance reading straight from the file,
//! mirroring the ELF and PE sides ([`crate::elfinfo`], [`crate::peinfo`]).
//! The label conventions follow the manifest's build-matrix spelling
//! (`docs/dataset.md`): `os=macos`, `arch` from the header's `cputype`
//! spelled the model's way (`x86_64`, `aarch64`), and the axes a Mach-O
//! container does not express stay `n/a` the way the manifest's windows rows
//! do. Ground truth for training still comes from the corpus manifest; the
//! values this module derives are what a user gets from `compid extract` on
//! a binary of unknown origin.
//!
//! Honesty notes:
//!
//! - `compiler_family`, `compiler_version`, and `opt_level` stay `unknown`:
//!   a Mach-O image carries no banner section the way ELF `.comment` does.
//! - `pie` is always `pie`: modern macOS requires position-independent main
//!   executables (dyld rejects anything else), so the container is PIE by
//!   construction. The historical `MH_PIE` flag is not consulted.
//! - `abi` and `static_linking` are `n/a`: nothing in the container says
//!   which libc flavor a build used, and macOS executables are always
//!   dynamically anchored to libSystem in practice.
//! - `lto` is `on` only when the image carries LLVM bitcode sections
//!   (`__LLVM,__bitcode` or `__LLVM,__bundle`), the direct analog of the PE
//!   side's `.gnu.lto_*` detection.
//! - `build_id` is the `LC_UUID` load command's 128-bit value in lowercase
//!   hex when one is present, else `none`.

use anyhow::{anyhow, bail, Context, Result};
use goblin::mach::constants::cputype;
use goblin::mach::header as mh;
use goblin::mach::load_command::CommandVariant;
use goblin::mach::MachO;
use std::path::Path;

use crate::elfinfo::SectionInfo;

/// The low 8 bits of a section's `flags` carry the section type.
const SECTION_TYPE_MASK: u32 = 0xff;
/// `S_ZEROFILL`: uninitialized data, no bytes in the file (the Mach-O
/// analog of ELF `SHT_NOBITS` and PE `IMAGE_SCN_CNT_UNINITIALIZED_DATA`).
const S_ZEROFILL: u32 = 0x1;
/// `S_GB_ZEROFILL`: the old 32-bit zero-fill-on-demand variant.
const S_GB_ZEROFILL: u32 = 0xc;
/// `S_THREAD_LOCAL_ZEROFILL`: zero-filled TLS, no bytes in the file.
const S_THREAD_LOCAL_ZEROFILL: u32 = 0x12;

/// Provenance labels read directly off a Mach-O file. Mirrors
/// [`crate::peinfo::PeLabels`]; see the module notes for what stays
/// `unknown` and why.
#[derive(Debug, Clone, PartialEq)]
pub struct MachLabels {
    pub compiler_family: String,
    pub compiler_version: String,
    pub opt_level: String,
    pub arch: String,
    pub os: String,
    pub abi: String,
    pub pie: String,
    pub lto: String,
    pub static_linking: String,
}

/// Everything feature extraction needs from one Mach-O binary.
#[derive(Debug, Clone)]
pub struct MachoInfo {
    pub labels: MachLabels,
    /// The `LC_UUID` value in lowercase hex, or `none`.
    pub build_id: String,
    /// Basenames of the dylibs the load commands name (e.g.
    /// `libSystem.B.dylib` from `/usr/lib/libSystem.B.dylib`). Counted as
    /// lowercase `cmt:` features during extraction, like PE's import DLLs.
    pub libraries: Vec<String>,
    /// Executable bytes of the `__TEXT,__text` section, if the file has one.
    pub text: Option<Vec<u8>>,
    /// Sections from every segment, named `__SEGMENT,__section` - Mach-O
    /// section names are only unique within their segment, so the segment
    /// name is part of the key.
    pub sections: Vec<SectionInfo>,
    /// Raw header `cputype` value, kept so callers can branch without
    /// re-parsing.
    pub cputype: u32,
    /// Raw header `filetype` value (`MH_EXECUTE`, `MH_DYLIB`, ...), kept so
    /// callers can tell image kinds apart without re-parsing.
    pub filetype: u32,
    /// The whole file, kept so per-section bytes (entropy, and anything
    /// feature extraction wants) stay available without a re-read.
    file: std::sync::Arc<Vec<u8>>,
}

impl MachoInfo {
    /// Copy of the file bytes for one section, clamped to the file extent.
    pub fn section_data(&self, offset: usize, size: u64) -> Vec<u8> {
        let end = offset.saturating_add(size as usize).min(self.file.len());
        if offset >= end {
            Vec::new()
        } else {
            self.file[offset..end].to_vec()
        }
    }

    /// True when the `__text` bytes can be fed to the x86-64 long-mode
    /// decoder. An arm64 (or any other) Mach-O skips the disassembly
    /// features instead of counting garbage, exactly like a non-x86-64 PE.
    pub fn disassembles_x86_64(&self) -> bool {
        self.cputype == cputype::CPU_TYPE_X86_64
    }
}

/// Read a file and parse it as Mach-O.
pub fn load(path: &Path) -> Result<MachoInfo> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse(std::sync::Arc::new(data)).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// Parse Mach-O bytes into a [`MachoInfo`]. 32-bit images are rejected with
/// a clear error; compid reads 64-bit Mach-O only.
pub fn parse(data: std::sync::Arc<Vec<u8>>) -> Result<MachoInfo> {
    if data.len() < 32 {
        bail!(
            "file is too short to be a Mach-O image ({} bytes)",
            data.len()
        );
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    match magic {
        mh::MH_MAGIC_64 | mh::MH_CIGAM_64 => {}
        mh::MH_MAGIC | mh::MH_CIGAM => bail!(
            "32-bit Mach-O images are not supported; compid reads 64-bit Mach-O (x86-64 or arm64)"
        ),
        _ => bail!("not a Mach-O object (missing MH_MAGIC)"),
    }

    let macho = MachO::parse(data.as_slice(), 0).context("Mach-O parse failed")?;

    let cpu = macho.header.cputype;
    let filetype = macho.header.filetype;

    // Dylib load commands carry full install names (/usr/lib/...); the
    // basename is the token that says which runtime a build touched.
    // goblin seeds `libs` with the literal "self" (dylib index 0 is the
    // image itself, not a load command), so that entry is dropped.
    let libraries: Vec<String> = macho
        .libs
        .iter()
        .filter(|l| ***l != *"self")
        .map(|l| dylib_basename(l).to_string())
        .collect();

    // Collect named sections with their extents. `size` is the memory size
    // the loader maps; zero-fill sections (and anything with no file
    // offset) have no bytes in the file, so entropy skips them.
    let mut sections: Vec<SectionInfo> = Vec::new();
    let mut text: Option<Vec<u8>> = None;
    let mut lto = false;
    for segment in &macho.segments {
        let segname_of_segment = segment.name().unwrap_or("?").to_string();
        for (sect, _) in segment
            .sections()
            .with_context(|| format!("sections of segment {segname_of_segment}"))?
        {
            let sectname = fixed_name(&sect.sectname);
            let segname = fixed_name(&sect.segname);
            if sectname.is_empty() {
                continue;
            }
            if segname == "__LLVM" && (sectname == "__bitcode" || sectname == "__bundle") {
                lto = true;
            }
            let stype = sect.flags & SECTION_TYPE_MASK;
            let no_bits = sect.offset == 0
                || matches!(stype, S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL);
            let offset = (sect.offset as usize).min(data.len());
            let end = offset.saturating_add(sect.size as usize).min(data.len());
            let bytes = if no_bits || offset >= end {
                Vec::new()
            } else {
                data[offset..end].to_vec()
            };
            if segname == "__TEXT" && sectname == "__text" && !no_bits {
                text = Some(bytes.clone());
            }
            sections.push(SectionInfo {
                name: format!("{segname},{sectname}"),
                offset,
                size: sect.size,
                no_bits,
                addr: sect.addr,
                executable: segname == "__TEXT",
            });
        }
    }

    let build_id = macho
        .load_commands
        .iter()
        .find_map(|lc| match &lc.command {
            CommandVariant::Uuid(u) => Some(u.uuid.iter().map(|b| format!("{b:02x}")).collect()),
            _ => None,
        })
        .unwrap_or_else(|| "none".to_string());

    Ok(MachoInfo {
        labels: MachLabels {
            compiler_family: "unknown".to_string(),
            compiler_version: "unknown".to_string(),
            opt_level: "unknown".to_string(),
            arch: cpu_name(cpu),
            os: "macos".to_string(),
            abi: "n/a".to_string(),
            pie: "pie".to_string(),
            lto: if lto { "on" } else { "off" }.to_string(),
            static_linking: "n/a".to_string(),
        },
        build_id,
        libraries,
        text,
        sections,
        cputype: cpu,
        filetype,
        file: data,
    })
}

/// Human-friendly architecture name from the header `cputype` value, spelled
/// the way the model's classes spell them.
fn cpu_name(cpu: u32) -> String {
    match cpu {
        cputype::CPU_TYPE_X86_64 => "x86_64".to_string(),
        cputype::CPU_TYPE_ARM64 => "aarch64".to_string(),
        cputype::CPU_TYPE_X86 => "x86".to_string(),
        cputype::CPU_TYPE_ARM => "arm".to_string(),
        other => format!("cputype{other:#010x}"),
    }
}

/// Basename of a dylib install name: the token after the last `/`.
fn dylib_basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// NUL-terminated fixed-width field ([u8; 16]) to a string.
fn fixed_name(raw: &[u8; 16]) -> String {
    String::from_utf8_lossy(raw)
        .trim_end_matches('\0')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_names_follow_the_model_spelling() {
        assert_eq!(cpu_name(cputype::CPU_TYPE_X86_64), "x86_64");
        assert_eq!(cpu_name(cputype::CPU_TYPE_ARM64), "aarch64");
        assert_eq!(cpu_name(cputype::CPU_TYPE_X86), "x86");
        assert_eq!(cpu_name(cputype::CPU_TYPE_ARM), "arm");
        assert_eq!(cpu_name(0x0100_0012), "cputype0x01000012");
    }

    #[test]
    fn dylib_names_strip_their_paths() {
        assert_eq!(
            dylib_basename("/usr/lib/libSystem.B.dylib"),
            "libSystem.B.dylib"
        );
        assert_eq!(dylib_basename("libfoo.dylib"), "libfoo.dylib");
        assert_eq!(
            dylib_basename("/usr/lib/system/libsystem_c.dylib"),
            "libsystem_c.dylib"
        );
    }

    #[test]
    fn fixed_width_names_lose_their_padding() {
        let mut raw = [0u8; 16];
        raw[..6].copy_from_slice(b"__text");
        assert_eq!(fixed_name(&raw), "__text");
        raw[..8].copy_from_slice(b"__TEXa__");
        assert_eq!(fixed_name(&raw), "__TEXa__");
    }

    #[test]
    fn cpu_type_constants_match_the_container_spec() {
        assert_eq!(cputype::CPU_TYPE_X86_64, 0x0100_0007);
        assert_eq!(cputype::CPU_TYPE_ARM64, 0x0100_000c);
    }
}
