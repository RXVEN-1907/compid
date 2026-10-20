//! ELF parsing: pull out the raw material feature extraction needs.
//!
//! Everything here is best-effort provenance reading straight from the file.
//! Ground-truth labels for training come from the corpus manifest; the values
//! this module derives (PIE-ness, linkage, toolchain family from `.comment`,
//! ...) are what a user gets from `compid extract` on a binary of unknown
//! origin.

use anyhow::{anyhow, bail, Context, Result};
use goblin::elf::header as eh;
use goblin::elf::section_header as sh;
use goblin::Object;
use std::collections::HashSet;
use std::path::Path;

/// One section worth remembering: name, where its bytes live, how big it is.
#[derive(Debug, Clone)]
pub struct SectionInfo {
    pub name: String,
    pub offset: usize,
    pub size: u64,
    /// true for SHT_NOBITS sections (`.bss`): a size is declared but no file
    /// bytes exist, so entropy cannot be computed.
    pub no_bits: bool,
}

/// Provenance labels read directly off an ELF file. `opt_level` is always
/// "unknown" here because optimization level is not recoverable from a
/// stripped binary; the corpus builder fills it from the build matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct ElfLabels {
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

/// One function symbol lifted from the ELF symbol table (unstripped ELF
/// only). `addr` is `st_value` exactly as the symbol table records it - a
/// virtual address in a linked binary, an offset within the containing
/// section in a relocatable object. `file_offset` is where the function's
/// first byte sits in the file, resolved through the containing section;
/// symbols whose range cannot be located (non-executable or out-of-bounds
/// section) are never added to the list. Names are raw symbol-table strings,
/// never demangled.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSymbol {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    pub file_offset: usize,
}

/// Everything feature extraction needs from one binary.
#[derive(Debug, Clone)]
pub struct ElfInfo {
    pub labels: ElfLabels,
    pub build_id: String,
    pub interpreter: Option<String>,
    /// Executable bytes of the `.text` section, if the file has section headers.
    pub text: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    pub sections: Vec<SectionInfo>,
    /// Function symbols from the symbol table (empty for a stripped ELF or
    /// an ELF whose symtab holds no usable functions).
    pub functions: Vec<FunctionSymbol>,
    /// true when the file carries an SHT_SYMTAB section at all; the
    /// stripped-versus-no-functions distinction for error reporting.
    pub has_symtab: bool,
    /// The whole file, kept so per-section bytes (entropy, and any section
    /// feature extraction wants) stay available without a re-read.
    file: std::sync::Arc<Vec<u8>>,
}

impl ElfInfo {
    /// Copy of the file bytes for one section, clamped to the file extent.
    pub fn section_data(&self, offset: usize, size: u64) -> Vec<u8> {
        self.file_slice(offset, size)
    }

    /// Copy of the file bytes for one `[offset, offset + size)` range,
    /// clamped to the file extent. Function feature extraction reads each
    /// function's bytes through this.
    pub fn file_slice(&self, offset: usize, size: u64) -> Vec<u8> {
        let end = offset.saturating_add(size as usize).min(self.file.len());
        if offset >= end {
            Vec::new()
        } else {
            self.file[offset..end].to_vec()
        }
    }
}

/// Read a file and parse it as ELF.
pub fn load(path: &Path) -> Result<ElfInfo> {
    let data = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if data.len() < 4 || &data[0..4] != b"\x7fELF" {
        bail!(
            "{} is not an ELF file (missing \\x7fELF magic); the ELF reader needs ELF bytes",
            path.display()
        );
    }
    parse(std::sync::Arc::new(data)).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// Parse ELF bytes into an [`ElfInfo`].
pub fn parse(data: std::sync::Arc<Vec<u8>>) -> Result<ElfInfo> {
    let elf = match Object::parse(data.as_slice()).context("ELF parse failed")? {
        Object::Elf(elf) => elf,
        _ => bail!("not an ELF object"),
    };

    let arch = machine_name(elf.header.e_machine);

    let interpreter = elf.interpreter.map(str::to_string);
    let interp = interpreter.as_deref().unwrap_or("");

    // OS + libc: the GNU ABI tag note and the interpreter path are the two
    // honest signals an ELF carries. Anything else stays "unknown"; the
    // corpus builder overrides from the build matrix.
    let has_gnu_abi_tag = section_name_exists(&elf, ".note.ABI-tag");
    let looks_linux = has_gnu_abi_tag || interp.contains("ld-linux") || interp.contains("ld-musl");
    let os = if looks_linux { "linux" } else { "unknown" }.to_string();
    let abi = if interp.contains("musl") {
        "musl".to_string()
    } else if interp.contains("ld-linux") || has_gnu_abi_tag {
        "glibc".to_string()
    } else {
        "unknown".to_string()
    };

    // PIE: ET_DYN with an interpreter is a PIE executable; ET_DYN without one
    // is a shared library or a static-PIE build; ET_EXEC is a no-pie build.
    let pie = match elf.header.e_type {
        eh::ET_EXEC => "no".to_string(),
        eh::ET_DYN => {
            if elf.interpreter.is_some() {
                "yes".to_string()
            } else {
                "shared".to_string()
            }
        }
        _ => "unknown".to_string(),
    };

    let static_linking = if elf.interpreter.is_some() {
        "dynamic".to_string()
    } else {
        "static".to_string()
    };

    // Collect named sections with their extents.
    let mut sections: Vec<SectionInfo> = Vec::new();
    let mut comment: Option<Vec<u8>> = None;
    let mut text: Option<Vec<u8>> = None;
    let mut lto = false;
    for hdr in &elf.section_headers {
        let name = elf
            .shdr_strtab
            .get_at(hdr.sh_name)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        if name.starts_with(".gnu.lto_") || name.starts_with(".llvm.lto") {
            lto = true;
        }
        let no_bits = hdr.sh_type == sh::SHT_NOBITS;
        let offset = (hdr.sh_offset as usize).min(data.len());
        let end = (hdr.sh_offset as usize)
            .saturating_add(hdr.sh_size as usize)
            .min(data.len());
        let bytes = if no_bits || offset >= end {
            Vec::new()
        } else {
            data[offset..end].to_vec()
        };
        if name == ".text" && !no_bits {
            text = Some(bytes.clone());
        }
        if name == ".comment" && !no_bits {
            comment = Some(bytes.clone());
        }
        sections.push(SectionInfo {
            name,
            offset,
            size: hdr.sh_size,
            no_bits,
        });
    }

    let (compiler_family, compiler_version) = comment.as_deref().map_or_else(
        || ("unknown".to_string(), "unknown".to_string()),
        |c| {
            let s = String::from_utf8_lossy(c);
            let family = if s.contains("clang version") {
                "clang"
            } else if s.contains("GCC:") {
                "gcc"
            } else {
                "unknown"
            };
            (family.to_string(), scan_version(&s))
        },
    );

    let build_id = find_build_id(&data, &elf);

    // Symbol `st_value` means different things in a relocatable object
    // (section offset) and a linked binary (virtual address).
    let is_rel = elf.header.e_type == eh::ET_REL;
    let (functions, has_symtab) = enumerate_functions(&elf, is_rel);

    Ok(ElfInfo {
        labels: ElfLabels {
            compiler_family,
            compiler_version,
            opt_level: "unknown".to_string(),
            arch,
            os,
            abi,
            pie,
            lto: if lto { "yes" } else { "no" }.to_string(),
            static_linking,
        },
        build_id,
        interpreter,
        text,
        comment,
        sections,
        functions,
        has_symtab,
        file: data,
    })
}

/// Collect STT_FUNC symbols with a nonzero size from the symbol table
/// (SHT_SYMTAB, not the dynamic one), deduplicated by `(st_value, st_size)`,
/// each resolved to a file offset through its containing executable
/// section. In a relocatable object (`ET_REL`) `st_value` is an offset
/// within the section; in a linked binary it is a virtual address matched
/// against the section's `sh_addr`. Empty for a stripped ELF.
fn enumerate_functions(elf: &goblin::elf::Elf, is_rel: bool) -> (Vec<FunctionSymbol>, bool) {
    let has_symtab = elf
        .section_headers
        .iter()
        .any(|hdr| hdr.sh_type == sh::SHT_SYMTAB);
    if !has_symtab {
        return (Vec::new(), false);
    }

    let mut functions: Vec<FunctionSymbol> = Vec::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    for sym in elf.syms.iter() {
        if !sym.is_function() || sym.st_size == 0 {
            continue;
        }
        if !seen.insert((sym.st_value, sym.st_size)) {
            continue; // same (address, size) range already listed
        }
        // Special section indexes (SHN_UNDEF, SHN_ABS, SHN_COMMON, ...)
        // never name an executable section; the range check below also
        // rejects them.
        let shndx = sym.st_shndx;
        if shndx == 0 || shndx >= elf.section_headers.len() {
            continue;
        }
        let hdr = &elf.section_headers[shndx];
        if hdr.sh_flags & u64::from(sh::SHF_EXECINSTR) == 0 {
            continue;
        }
        let file_offset = if is_rel {
            // st_value is the offset within the section; the declared range
            // must stay inside it.
            if sym.st_value.saturating_add(sym.st_size) > hdr.sh_size {
                continue; // crafted symbol: range runs past the section
            }
            hdr.sh_offset as usize + sym.st_value as usize
        } else {
            let lo = hdr.sh_addr;
            let hi = lo.saturating_add(hdr.sh_size);
            if sym.st_value >= lo && sym.st_value < hi {
                // The declared byte range must stay inside the section too,
                // or a crafted symbol could have us read past it.
                if sym.st_value.saturating_add(sym.st_size) > hi {
                    continue;
                }
                hdr.sh_offset as usize + (sym.st_value - lo) as usize
            } else {
                continue;
            }
        };
        let name = elf.strtab.get_at(sym.st_name).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        functions.push(FunctionSymbol {
            name,
            addr: sym.st_value,
            size: sym.st_size,
            file_offset,
        });
    }
    functions.sort_by(|a, b| a.addr.cmp(&b.addr).then_with(|| a.name.cmp(&b.name)));
    (functions, has_symtab)
}

fn section_name_exists(elf: &goblin::elf::Elf, want: &str) -> bool {
    elf.section_headers
        .iter()
        .any(|hdr| elf.shdr_strtab.get_at(hdr.sh_name) == Some(want))
}

/// Human-friendly architecture name from `e_machine`.
fn machine_name(e_machine: u16) -> String {
    match e_machine {
        eh::EM_X86_64 => "x86-64".to_string(),
        eh::EM_386 => "x86".to_string(),
        eh::EM_AARCH64 => "aarch64".to_string(),
        eh::EM_ARM => "arm".to_string(),
        eh::EM_RISCV => "riscv".to_string(),
        eh::EM_PPC64 => "ppc64".to_string(),
        eh::EM_S390 => "s390x".to_string(),
        other => format!("em{other}"),
    }
}

/// First `\d+(\.\d+)+`-shaped run in a toolchain comment, e.g. "13.2.0" out
/// of "GCC: (Ubuntu 13.2.0-23ubuntu4) 13.2.0". No regex dependency needed.
fn scan_version(s: &str) -> String {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            let mut j = i;
            let mut dots = 0;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                if b[j] == b'.' {
                    dots += 1;
                }
                j += 1;
            }
            let mut end = j;
            while end > start && b[end - 1] == b'.' {
                end -= 1;
            }
            if dots >= 1 && end > start {
                return s[start..end].to_string();
            }
            i = j;
        } else {
            i += 1;
        }
    }
    "unknown".to_string()
}

/// Hex string of the GNU build id, or "none" when the binary has none.
fn find_build_id(data: &[u8], elf: &goblin::elf::Elf) -> String {
    for hdr in &elf.section_headers {
        if elf.shdr_strtab.get_at(hdr.sh_name) != Some(".note.gnu.build-id") {
            continue;
        }
        let off = (hdr.sh_offset as usize).min(data.len());
        let end = (hdr.sh_offset as usize)
            .saturating_add(hdr.sh_size as usize)
            .min(data.len());
        if let Some(hex) = parse_gnu_build_id(&data[off..end]) {
            return hex;
        }
    }
    "none".to_string()
}

/// ELF note layout: namesz, descsz, type (u32 each), name padded to 4, desc.
/// The GNU build id note has name "GNU" and type NT_GNU_BUILD_ID (3).
fn parse_gnu_build_id(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 {
        return None;
    }
    let namesz = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    let descsz = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let ntype = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    if namesz != 4 || ntype != 3 || bytes.len() < 12 + 4 + descsz {
        return None;
    }
    if &bytes[12..16] != b"GNU\0" {
        return None;
    }
    let desc = &bytes[16..16 + descsz];
    Some(desc.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_scan_finds_dot_versions() {
        assert_eq!(
            scan_version("GCC: (Ubuntu 13.2.0-23ubuntu4) 13.2.0"),
            "13.2.0"
        );
        assert_eq!(scan_version("clang version 18.1.3"), "18.1.3");
        assert_eq!(scan_version("no version here"), "unknown");
    }

    #[test]
    fn machine_names_are_friendly() {
        assert_eq!(machine_name(eh::EM_X86_64), "x86-64");
        assert_eq!(machine_name(eh::EM_AARCH64), "aarch64");
        assert_eq!(machine_name(9999), "em9999");
    }
}
