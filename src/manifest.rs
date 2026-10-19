//! Manifest CSV parsing.
//!
//! The corpus manifest is a single CSV with a fixed header:
//!
//! ```text
//! build_id,source_id,toolchain,compiler_family,compiler_version,opt_level,arch,os,abi,pie,lto,static,path,sha256
//! ```
//!
//! The parser below handles RFC-4180-style quoting (`"a,b"` and `""`
//! escapes) so paths with commas still work. Multi-line quoted fields are
//! not supported; build paths never need them.

use anyhow::{bail, Context, Result};

/// The exact column order of a manifest CSV.
pub const MANIFEST_COLUMNS: &[&str] = &[
    "build_id",
    "source_id",
    "toolchain",
    "compiler_family",
    "compiler_version",
    "opt_level",
    "arch",
    "os",
    "abi",
    "pie",
    "lto",
    "static",
    "path",
    "sha256",
];

/// Label columns shared between the manifest and the feature JSON schema.
pub const LABEL_COLUMNS: &[&str] = &[
    "compiler_family",
    "compiler_version",
    "opt_level",
    "arch",
    "os",
    "abi",
    "pie",
    "lto",
    "static",
];

/// One parsed manifest row. Missing cells read as "unknown".
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestRow {
    pub build_id: String,
    pub source_id: String,
    pub toolchain: String,
    pub compiler_family: String,
    pub compiler_version: String,
    pub opt_level: String,
    pub arch: String,
    pub os: String,
    pub abi: String,
    pub pie: String,
    pub lto: String,
    pub static_linking: String,
    pub path: String,
    pub sha256: String,
}

impl ManifestRow {
    fn from_fields(header: &[String], fields: &[String]) -> ManifestRow {
        let get = |name: &str| -> String {
            header
                .iter()
                .position(|h| h == name)
                .and_then(|i| fields.get(i))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        };
        ManifestRow {
            build_id: get("build_id"),
            source_id: get("source_id"),
            toolchain: get("toolchain"),
            compiler_family: get("compiler_family"),
            compiler_version: get("compiler_version"),
            opt_level: get("opt_level"),
            arch: get("arch"),
            os: get("os"),
            abi: get("abi"),
            pie: get("pie"),
            lto: get("lto"),
            static_linking: get("static"),
            path: get("path"),
            sha256: get("sha256"),
        }
    }
}

/// A whole manifest: header row plus raw field rows, so generic tools can
/// iterate any column.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub header: Vec<String>,
    pub rows: Vec<ManifestRow>,
}

/// Parse manifest CSV text.
pub fn parse(text: &str) -> Result<Manifest> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines
        .next()
        .context("manifest CSV is empty (no header row)")?;
    let header: Vec<String> = split_csv_line(header_line)
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    if !header.iter().any(|h| h == "path") {
        bail!("manifest header has no \"path\" column");
    }
    if !header.iter().any(|h| h == "build_id") {
        bail!("manifest header has no \"build_id\" column");
    }

    let mut rows = Vec::new();
    for (n, line) in lines.enumerate() {
        let fields = split_csv_line(line);
        if fields.len() != header.len() {
            bail!(
                "manifest line {}: {} fields, header has {}",
                n + 2,
                fields.len(),
                header.len()
            );
        }
        rows.push(ManifestRow::from_fields(&header, &fields));
    }
    Ok(Manifest { header, rows })
}

/// Split one CSV line into fields, honoring double-quoted fields and `""`
/// escapes.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        "build_id,source_id,toolchain,compiler_family,compiler_version,opt_level,arch,os,abi,pie,lto,static,path,sha256\n",
        "b1,src1,gcc-13,gcc,13.2.0,O2,x86-64,linux,glibc,yes,no,dynamic,binaries/b1,abc\n",
        "b2,src2,clang-18,clang,18.1.3,O0,x86-64,linux,glibc,yes,no,dynamic,\"binaries/b,2\",def\n",
        "b3,src1,gcc-13,gcc,13.2.0,O2,x86-64,linux,musl,no,yes,static,binaries/b3,ghi\n",
    );

    #[test]
    fn parses_the_contract_header_and_rows() {
        let m = parse(SAMPLE).expect("sample parses");
        assert_eq!(m.rows.len(), 3);
        let r = &m.rows[0];
        assert_eq!(r.build_id, "b1");
        assert_eq!(r.compiler_family, "gcc");
        assert_eq!(r.opt_level, "O2");
        assert_eq!(r.static_linking, "dynamic");
        assert_eq!(r.path, "binaries/b1");
        assert_eq!(r.sha256, "abc");
    }

    #[test]
    fn quoted_fields_with_commas_parse() {
        let m = parse(SAMPLE).expect("sample parses");
        assert_eq!(m.rows[1].path, "binaries/b,2");
    }

    #[test]
    fn missing_cells_become_unknown() {
        let text = "build_id,path,os\nb9,binaries/x,\n";
        let m = parse(text).expect("parses");
        assert_eq!(m.rows[0].build_id, "b9");
        assert_eq!(m.rows[0].os, "unknown");
        assert_eq!(m.rows[0].compiler_family, "unknown");
    }

    #[test]
    fn ragged_rows_are_rejected() {
        let text = "build_id,path\nb1,binaries/x,extra\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn headerless_or_wrong_header_rejected() {
        assert!(parse("").is_err());
        assert!(parse("a,b\n1,2\n").is_err());
    }
}
