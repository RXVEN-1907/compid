//! Feature extraction tests against a synthetic PE32+ built byte-by-byte in
//! the test, so Windows support is exercised on every `cargo test` run even
//! when `dataset/binaries/` has no committed binaries. The image is the
//! minimum goblin parses happily: DOS header, PE32+ optional header, two
//! sections (.text, .rdata), and an import table naming KERNEL32.dll.

use compid::{features, peinfo};
use std::sync::Arc;

const FILE_ALIGN: u32 = 0x200;
const SECT_ALIGN: u32 = 0x1000;
const HEADERS_SIZE: u32 = 0x400;
const TEXT_RAW_PTR: u32 = 0x400;
const RDATA_RAW_PTR: u32 = 0x600;
const RDATA_VA: u32 = 0x2000;

fn align_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

/// The `.rdata` payload: one import descriptor (with the standard null
/// terminator), its by-name lookup table, the hint/name entry, the import
/// address table, and the DLL name - all RVAs relative to `RDATA_VA`,
/// patched in as they are laid out.
fn import_rdata(dll: &str) -> Vec<u8> {
    let mut b = Vec::new();

    // IMAGE_IMPORT_DESCRIPTOR: original first thunk, time/date stamp,
    // forwarder chain, name, first thunk.
    let lookup_slot = b.len();
    b.extend_from_slice(&0u32.to_le_bytes()); // lookup RVA, patched below
    b.extend_from_slice(&0u32.to_le_bytes()); // time/date stamp
    b.extend_from_slice(&0u32.to_le_bytes()); // forwarder chain
    let name_slot = b.len();
    b.extend_from_slice(&0u32.to_le_bytes()); // name RVA, patched below
    let iat_slot = b.len();
    b.extend_from_slice(&0u32.to_le_bytes()); // first thunk, patched below
    b.extend_from_slice(&[0u8; 20]); // null descriptor terminator

    let lookup_rva = RDATA_VA + b.len() as u32;
    let hint_name_rva = lookup_rva + 16; // one u64 entry + terminator
    let iat_rva = hint_name_rva + 2 + 11 + 1; // hint u16 + name + NUL
    let name_rva = iat_rva + 16; // one u64 entry + terminator

    b[lookup_slot..lookup_slot + 4].copy_from_slice(&lookup_rva.to_le_bytes());
    b[name_slot..name_slot + 4].copy_from_slice(&name_rva.to_le_bytes());
    b[iat_slot..iat_slot + 4].copy_from_slice(&iat_rva.to_le_bytes());

    // Import lookup table: one by-name entry, then the zero terminator.
    b.extend_from_slice(&(hint_name_rva as u64).to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    // Hint/name table: hint 0, "ExitProcess".
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(b"ExitProcess\0");
    // Import address table: one entry, then the zero terminator.
    b.extend_from_slice(&(hint_name_rva as u64).to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    // The DLL name itself.
    b.extend_from_slice(dll.as_bytes());
    b.push(0);
    b
}

/// Minimum viable PE32+ executable: two sections and one import.
fn tiny_pe_exec(machine: u16, text: &[u8], dll: &str) -> Vec<u8> {
    let imports = import_rdata(dll);
    let text_raw = align_up(text.len() as u32, FILE_ALIGN);
    let rdata_raw = align_up(imports.len() as u32, FILE_ALIGN);
    let image_size = align_up(RDATA_VA + rdata_raw, SECT_ALIGN);

    let mut out = Vec::new();

    // DOS header: magic at 0, e_lfanew at 0x3c.
    out.extend_from_slice(b"MZ");
    out.extend_from_slice(&[0u8; 58]);
    out.extend_from_slice(&0x40u32.to_le_bytes());
    assert_eq!(out.len(), 64);

    // PE signature.
    out.extend_from_slice(b"PE\0\0");

    // COFF header.
    out.extend_from_slice(&machine.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // number of sections
    out.extend_from_slice(&0u32.to_le_bytes()); // time/date stamp
    out.extend_from_slice(&0u32.to_le_bytes()); // pointer to symbol table
    out.extend_from_slice(&0u32.to_le_bytes()); // number of symbols
    out.extend_from_slice(&240u16.to_le_bytes()); // size of optional header
    out.extend_from_slice(&0x0022u16.to_le_bytes()); // executable image, large address aware

    // Optional header, PE32+ flavor.
    out.extend_from_slice(&0x020bu16.to_le_bytes()); // PE32+ magic
    out.push(14); // major linker version
    out.push(0); // minor linker version
    out.extend_from_slice(&text_raw.to_le_bytes()); // size of code
    out.extend_from_slice(&rdata_raw.to_le_bytes()); // size of initialized data
    out.extend_from_slice(&0u32.to_le_bytes()); // size of uninitialized data
    out.extend_from_slice(&0x1000u32.to_le_bytes()); // entry point RVA
    out.extend_from_slice(&0x1000u32.to_le_bytes()); // base of code
    out.extend_from_slice(&0x140000000u64.to_le_bytes()); // image base
    out.extend_from_slice(&SECT_ALIGN.to_le_bytes());
    out.extend_from_slice(&FILE_ALIGN.to_le_bytes());
    out.extend_from_slice(&6u16.to_le_bytes()); // major OS version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor OS version
    out.extend_from_slice(&0u16.to_le_bytes()); // major image version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor image version
    out.extend_from_slice(&6u16.to_le_bytes()); // major subsystem version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor subsystem version
    out.extend_from_slice(&0u32.to_le_bytes()); // win32 version value
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&HEADERS_SIZE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // checksum
    out.extend_from_slice(&3u16.to_le_bytes()); // subsystem: console
    out.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    out.extend_from_slice(&0x10_0000u64.to_le_bytes()); // stack reserve
    out.extend_from_slice(&0x1000u64.to_le_bytes()); // stack commit
    out.extend_from_slice(&0x10_0000u64.to_le_bytes()); // heap reserve
    out.extend_from_slice(&0x1000u64.to_le_bytes()); // heap commit
    out.extend_from_slice(&0u32.to_le_bytes()); // loader flags
    out.extend_from_slice(&16u32.to_le_bytes()); // number of RVA and sizes

    // Data directories: export zero, import table, the rest zero.
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&RDATA_VA.to_le_bytes());
    out.extend_from_slice(&(imports.len() as u32).to_le_bytes());
    for _ in 2..16 {
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    assert_eq!(out.len(), 64 + 4 + 20 + 240);

    // Section headers: name, virtual size/address, raw size/pointer,
    // relocation and line-number fields, characteristics.
    let mut section = |name: &[u8; 8], va: u32, raw: u32, ptr: u32, chars: u32| {
        out.extend_from_slice(name);
        out.extend_from_slice(&raw.to_le_bytes()); // virtual size = raw
        out.extend_from_slice(&va.to_le_bytes());
        out.extend_from_slice(&raw.to_le_bytes());
        out.extend_from_slice(&ptr.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&chars.to_le_bytes());
    };
    section(
        b".text\0\0\0",
        0x1000,
        text_raw,
        TEXT_RAW_PTR,
        0x6000_0020, // code, execute, read
    );
    section(
        b".rdata\0\0",
        RDATA_VA,
        rdata_raw,
        RDATA_RAW_PTR,
        0x4000_0040, // initialized data, read
    );

    // Pad the headers to their declared extent, then lay the sections out.
    out.resize(HEADERS_SIZE as usize, 0);
    out.resize(TEXT_RAW_PTR as usize, 0);
    out.extend_from_slice(text);
    out.resize(RDATA_RAW_PTR as usize, 0);
    out.extend_from_slice(&imports);
    out.resize((RDATA_RAW_PTR + rdata_raw) as usize, 0);
    out
}

fn extract_synthetic(machine: u16, text: &[u8]) -> features::ExtractedFeatures {
    let bytes = tiny_pe_exec(machine, text, "KERNEL32.dll");
    let info = peinfo::parse(Arc::new(bytes)).expect("synthetic PE parses");
    features::extract_pe(&info).expect("synthetic PE extracts")
}

#[test]
fn synthetic_pe_carries_windows_labels() {
    let info = peinfo::parse(Arc::new(tiny_pe_exec(0x8664, &[0x90; 128], "KERNEL32.dll")))
        .expect("synthetic PE parses");

    assert_eq!(info.labels.os, "windows");
    assert_eq!(info.labels.arch, "x86_64");
    assert_eq!(info.labels.pie, "n/a");
    assert_eq!(info.labels.lto, "off");
    assert_eq!(info.labels.static_linking, "n/a");
    // Kernel32 alone does not decide an ABI flavor.
    assert_eq!(info.labels.abi, "unknown");
    assert_eq!(info.build_id, "none");
    assert_eq!(info.libraries, vec!["KERNEL32.dll"]);
}

#[test]
fn synthetic_pe_features_match_the_schema() {
    // 512 single-byte nops fill the aligned .text exactly, so the window
    // count is not affected by the zero padding after the section's code.
    let ef = extract_synthetic(0x8664, &[0x90; 512]);

    assert!(!ef.features.is_empty());
    // Import DLL names land in the cmt: namespace, lowercased.
    assert_eq!(ef.features.get("cmt:kernel32.dll"), Some(&1));
    // 512 bytes of single-byte instructions make eight clean 64-byte windows.
    assert_eq!(ef.features.get("meta:windows"), Some(&8));
    // Nop n-grams and section geometry both show up.
    assert_eq!(ef.features.get("ng:909090"), Some(&510));
    assert!(ef.features.contains_key("sec:.text"));
    assert!(ef.features.contains_key("ent:.text"));

    // The wire labels mirror the manifest's windows-row conventions.
    let json = serde_json::to_string(&ef).expect("serializes");
    assert!(json.contains("\"os\":\"windows\""), "{json}");
    assert!(json.contains("\"arch\":\"x86_64\""), "{json}");
    assert!(json.contains("\"pie\":\"n/a\""), "{json}");
    assert!(json.contains("\"static\":\"n/a\""), "{json}");
    assert!(json.starts_with("{\"build_id\":"), "{json}");
}

#[test]
fn aarch64_pe_skips_disassembly_gracefully() {
    let ef = extract_synthetic(0xaa64, &[0x90; 512]);

    assert_eq!(ef.labels.arch, "aarch64");
    // No x86-64 decoder run: no mnemonics, registers, immediates, windows.
    assert_eq!(ef.features.get("meta:windows"), Some(&0));
    assert!(!ef.features.keys().any(|k| k.starts_with("mn:")));
    assert!(!ef.features.keys().any(|k| k.starts_with("rg:")));
    assert!(!ef.features.keys().any(|k| k.starts_with("cn:")));
    // Byte-level features still flow.
    assert_eq!(ef.features.get("ng:909090"), Some(&510));
    assert!(ef.features.contains_key("sec:.text"));
}

#[test]
fn dispatcher_routes_by_magic() {
    let dir = std::env::temp_dir();
    let pe_path = dir.join("compid-synthetic-pe-test.exe");
    std::fs::write(&pe_path, tiny_pe_exec(0x8664, &[0x90; 64], "KERNEL32.dll"))
        .expect("write temp PE");

    let ef = compid::extract_binary(&pe_path).expect("dispatcher routes MZ to the PE reader");
    assert_eq!(ef.labels.os, "windows");
    std::fs::remove_file(&pe_path).ok();

    // Anything without ELF or MZ magic is rejected with a clear error.
    let junk_path = dir.join("compid-synthetic-pe-test.bin");
    std::fs::write(&junk_path, b"definitely not a binary").expect("write temp junk");
    let err = compid::extract_binary(&junk_path).expect_err("junk is rejected");
    assert!(err.to_string().contains("unsupported binary format"));
    std::fs::remove_file(&junk_path).ok();
}
