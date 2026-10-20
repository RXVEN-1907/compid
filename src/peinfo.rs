//! PE (Windows) parsing: pull out the raw material feature extraction needs.
//!
//! Everything here is best-effort provenance reading straight from the file,
//! mirroring the ELF side ([`crate::elfinfo`]). The label conventions follow
//! the manifest's windows rows (`docs/dataset.md`): `os=windows`, `arch`
//! spelled the COFF way (`x86_64`, where the ELF side spells the same
//! machine `x86-64`), and the PIE/linkage axes the seed's Windows builds do
//! not track stay `n/a`. Ground truth for training still comes from the
//! corpus manifest; the values this module derives are what a user gets
//! from `compid extract` on a binary of unknown origin.
//!
//! Honesty notes:
//!
//! - `compiler_family` and `compiler_version` stay `unknown`: PE images
//!   carry no banner section the way ELF `.comment` does.
//! - `abi` is guessed only from runtime DLLs the import table names (mingw's
//!   `libgcc*` family means `gnu`, MSVC's `vcruntime*`/`msvcr*` means
//!   `msvc`); anything ambiguous stays `unknown`. The manifest records the
//!   build-matrix value (e.g. `gnu` for the zig-cc windows-gnu rows), and
//!   `compid batch` replaces these labels with it.
//! - `build_id` is the CodeView PDB 7.0 signature GUID when a debug
//!   directory carries one, else `none`.
//! - Function symbols come from the export directory when the image carries
//!   one (see [`PeInfo::functions`]). The export table records no sizes, so
//!   each export's size is derived: the delta to the next export's RVA in
//!   the same section, or the section end for the last one. A plain
//!   executable carries no export directory at all; a plain x86-64
//!   executable falls back to the stripped-binary prologue heuristic over
//!   the section table's executable sections (RVAs, via
//!   `functions::heuristic_functions_pe`), while a non-x86-64 one reports
//!   an explicit reason.

use anyhow::{anyhow, bail, Context, Result};
use goblin::pe::header as coff;
use goblin::Object;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::elfinfo::{FunctionSymbol, SectionInfo};

/// IMAGE_SCN_CNT_UNINITIALIZED_DATA: the section occupies memory but holds
/// no bytes in the file (the PE analog of ELF `SHT_NOBITS`).
const SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;

/// IMAGE_SCN_MEM_EXECUTE: the section holds machine code.
const SCN_MEM_EXECUTE: u32 = 0x2000_0000;

/// Provenance labels read directly off a PE file. Mirrors
/// [`crate::elfinfo::ElfLabels`]; see the module notes for what stays
/// `unknown` and why.
#[derive(Debug, Clone, PartialEq)]
pub struct PeLabels {
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

/// Everything feature extraction needs from one PE binary.
#[derive(Debug, Clone)]
pub struct PeInfo {
    pub labels: PeLabels,
    pub build_id: String,
    /// DLL names from the import table (the parser sorts and deduplicates
    /// them). Counted as lowercase `cmt:` features during extraction.
    pub libraries: Vec<String>,
    /// Executable bytes of the `.text` section, if the file has one.
    pub text: Option<Vec<u8>>,
    pub sections: Vec<SectionInfo>,
    /// Raw COFF `machine` value, kept so callers can branch without
    /// re-parsing.
    pub machine: u16,
    /// Exported functions from the export directory (empty when the image
    /// has no export table). Only named, non-forwarded exports whose RVA
    /// lands in an executable, byte-backed section are listed,
    /// deduplicated by `(rva, size)`; names are the raw export-directory
    /// strings, never demangled. The export table records no sizes, so
    /// `size` is derived: the delta to the next export's RVA in the same
    /// section, or the section end for the last one.
    pub functions: Vec<FunctionSymbol>,
    /// true when the file carries an export data directory at all; the
    /// no-export-table-versus-empty-table distinction for error reporting.
    pub has_exports: bool,
    /// The whole file, kept so per-section bytes (entropy, and anything
    /// feature extraction wants) stay available without a re-read.
    file: std::sync::Arc<Vec<u8>>,
}

impl PeInfo {
    /// Copy of the file bytes for one section, clamped to the file extent.
    pub fn section_data(&self, offset: usize, size: u64) -> Vec<u8> {
        let end = offset.saturating_add(size as usize).min(self.file.len());
        if offset >= end {
            Vec::new()
        } else {
            self.file[offset..end].to_vec()
        }
    }

    /// True when the `.text` bytes can be fed to the x86-64 long-mode
    /// decoder. x86-64 is the only PE flavor the seed matrix produces;
    /// any other machine (aarch64 if it ever appears) skips the
    /// disassembly features instead of counting garbage.
    pub fn disassembles_x86_64(&self) -> bool {
        self.machine == coff::COFF_MACHINE_X86_64
    }
}

/// Read a file and parse it as PE.
pub fn load(path: &Path) -> Result<PeInfo> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if data.len() < 2 || &data[0..2] != b"MZ" {
        bail!("{} is not a PE file (missing MZ magic)", path.display());
    }
    parse(std::sync::Arc::new(data)).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// Parse PE bytes into a [`PeInfo`].
pub fn parse(data: std::sync::Arc<Vec<u8>>) -> Result<PeInfo> {
    let pe = match Object::parse(data.as_slice()).context("PE parse failed")? {
        Object::PE(pe) => pe,
        _ => bail!("not a PE object"),
    };

    let machine = pe.header.coff_header.machine;
    let arch = machine_name(machine);

    // A PE is a Windows image by construction; `os=windows` is a property of
    // the container, not a guess.
    let os = "windows".to_string();

    // ABI flavor from the runtime DLLs the import table names.
    let libraries: Vec<String> = pe.libraries.iter().map(|s| s.to_string()).collect();
    let abi = abi_from_libraries(&libraries).to_string();

    // Collect named sections with their extents. `size` counts the bytes the
    // file actually holds (raw data); uninitialized sections fall back to
    // their declared virtual size so section geometry still shows up.
    let mut sections: Vec<SectionInfo> = Vec::new();
    let mut text: Option<Vec<u8>> = None;
    let mut lto = false;
    for hdr in &pe.sections {
        let name = hdr.name().map(str::to_string).unwrap_or_else(|_| {
            String::from_utf8_lossy(&hdr.name)
                .trim_end_matches('\0')
                .to_string()
        });
        if name.is_empty() {
            continue;
        }
        if name.starts_with(".gnu.lto_") || name.starts_with(".llvm.lto") {
            lto = true;
        }
        let raw_size = hdr.size_of_raw_data as usize;
        let no_bits = raw_size == 0 || hdr.characteristics & SCN_CNT_UNINITIALIZED_DATA != 0;
        let offset = (hdr.pointer_to_raw_data as usize).min(data.len());
        let end = (hdr.pointer_to_raw_data as usize)
            .saturating_add(raw_size)
            .min(data.len());
        let bytes = if no_bits || offset >= end {
            Vec::new()
        } else {
            data[offset..end].to_vec()
        };
        let size = if raw_size > 0 {
            raw_size as u64
        } else {
            hdr.virtual_size as u64
        };
        if name == ".text" && !no_bits {
            text = Some(bytes.clone());
        }
        sections.push(SectionInfo {
            name,
            offset,
            size,
            no_bits,
            addr: u64::from(hdr.virtual_address),
            executable: hdr.characteristics & SCN_MEM_EXECUTE != 0,
        });
    }

    let build_id = pe.debug_data.as_ref().and_then(|d| d.guid()).map_or_else(
        || "none".to_string(),
        |guid| guid.iter().map(|b| format!("{b:02x}")).collect(),
    );

    let has_exports = pe.export_data.is_some();
    let functions = enumerate_exported_functions(&pe, &sections);

    Ok(PeInfo {
        labels: PeLabels {
            compiler_family: "unknown".to_string(),
            compiler_version: "unknown".to_string(),
            opt_level: "unknown".to_string(),
            arch,
            os,
            abi,
            pie: "n/a".to_string(),
            lto: if lto { "on" } else { "off" }.to_string(),
            static_linking: "n/a".to_string(),
        },
        build_id,
        libraries,
        text,
        sections,
        machine,
        functions,
        has_exports,
        file: data,
    })
}

/// Collect exported functions from the export directory. Only named,
/// non-forwarded exports (`name` present, `reexport` empty - a forwarder
/// RVA points at a string, not code) whose RVA lands inside an executable,
/// byte-backed section are kept; data exports and exports into
/// uninitialized sections never reach the list. The RVA is the function's
/// address (the manifest-convention addr the ELF side records as
/// `st_value`), resolved to a file offset through its containing section -
/// the same containment rule the ELF path applies to symbol values.
///
/// The export table records no sizes, so each export's size is derived: the
/// delta to the next export's RVA in the same section, or the section end
/// for that section's last export (the section-clamped estimate). Two
/// exports sharing one RVA (ordinal aliases of one function) both take that
/// derived size and then collapse by `(rva, size)` with the
/// alphabetically-first name winning, mirroring the ELF path's
/// first-symbol-wins dedupe. Entries with a derived size of zero are
/// skipped (unsizeable). Names stay raw export-directory strings, never
/// demangled.
fn enumerate_exported_functions(
    pe: &goblin::pe::PE,
    sections: &[SectionInfo],
) -> Vec<FunctionSymbol> {
    // One entry per named, non-forwarded export in an executable section:
    // `(rva, name)`, grouped by the section that contains the RVA.
    let mut by_section: BTreeMap<usize, Vec<(u64, String)>> = BTreeMap::new();
    for export in &pe.exports {
        if export.reexport.is_some() {
            continue; // a forwarder names a string, not this image's code
        }
        let Some(name) = export.name else {
            continue; // ordinal-only export: no name to report
        };
        if name.is_empty() {
            continue;
        }
        let rva = u64::from(export.rva as u32);
        let holding = sections.iter().position(|s| {
            !s.no_bits
                && s.executable
                && s.size > 0
                && rva >= s.addr
                && rva < s.addr.saturating_add(s.size)
        });
        if let Some(idx) = holding {
            by_section
                .entry(idx)
                .or_default()
                .push((rva, name.to_string()));
        }
    }

    let mut functions: Vec<FunctionSymbol> = Vec::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    for (idx, mut sites) in by_section {
        sites.sort();
        let section = &sections[idx];
        let hi = section.addr.saturating_add(section.size);
        // The distinct RVAs in this section, ascending: consecutive
        // distinct deltas are the derived sizes.
        let mut distinct: Vec<u64> = sites.iter().map(|(rva, _)| *rva).collect();
        distinct.dedup();
        for (k, &rva) in distinct.iter().enumerate() {
            let next = distinct.get(k + 1).copied().unwrap_or(hi);
            let size = next.min(hi).saturating_sub(rva);
            if size == 0 {
                continue; // unsizeable: no range to fingerprint
            }
            for (_, name) in sites.iter().filter(|(r, _)| *r == rva) {
                if !seen.insert((rva, size)) {
                    continue; // alias of an already-listed range
                }
                let delta = usize::try_from(rva - section.addr).unwrap_or(usize::MAX);
                functions.push(FunctionSymbol {
                    name: name.clone(),
                    addr: rva,
                    size,
                    file_offset: section.offset.saturating_add(delta),
                });
            }
        }
    }
    functions.sort_by(|a, b| a.addr.cmp(&b.addr).then_with(|| a.name.cmp(&b.name)));
    functions
}

/// Human-friendly architecture name from the COFF `machine` value, spelled
/// the way the manifest's windows rows spell them.
fn machine_name(machine: u16) -> String {
    match machine {
        coff::COFF_MACHINE_X86_64 => "x86_64".to_string(),
        coff::COFF_MACHINE_ARM64 => "aarch64".to_string(),
        coff::COFF_MACHINE_X86 => "x86".to_string(),
        other => format!("machine{other:#06x}"),
    }
}

/// ABI flavor guess from import-table DLL names. Runtime DLLs unique to one
/// world say that world's name: mingw's `libgcc*`/`libstdc++*`/
/// `libwinpthread*` mean a gnu-flavored toolchain, `vcruntime*`/`msvcr*`/
/// `msvcp*` mean MSVC. Kernel32 plus the UCRT API sets alone (what both a
/// statically linked mingw runtime and an MSVC build show) does not decide,
/// so it stays unknown.
fn abi_from_libraries(libraries: &[String]) -> &'static str {
    let lower: Vec<String> = libraries.iter().map(|l| l.to_ascii_lowercase()).collect();
    let any = |needle: &str| lower.iter().any(|l| l.contains(needle));
    if any("libgcc") || any("libstdc++") || any("libwinpthread") || any("mingw") {
        "gnu"
    } else if any("vcruntime") || any("msvcr") || any("msvcp") {
        "msvc"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_names_follow_the_manifest_spelling() {
        assert_eq!(machine_name(coff::COFF_MACHINE_X86_64), "x86_64");
        assert_eq!(machine_name(coff::COFF_MACHINE_ARM64), "aarch64");
        assert_eq!(machine_name(coff::COFF_MACHINE_X86), "x86");
        assert_eq!(machine_name(0x5032), "machine0x5032");
    }

    #[test]
    fn abi_guess_only_bites_on_distinctive_runtimes() {
        assert_eq!(
            abi_from_libraries(&["libgcc_s_seh-1.dll".to_string()]),
            "gnu"
        );
        assert_eq!(
            abi_from_libraries(&["LIBWINPTHREAD-1.dll".to_string()]),
            "gnu"
        );
        assert_eq!(
            abi_from_libraries(&["vcruntime140.dll".to_string()]),
            "msvc"
        );
        let ambiguous = [
            "KERNEL32.dll".to_string(),
            "api-ms-win-crt-stdio-l1-1-0.dll".to_string(),
        ];
        assert_eq!(abi_from_libraries(&ambiguous), "unknown");
        assert_eq!(abi_from_libraries(&[]), "unknown");
    }
}
