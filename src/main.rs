//! compid - binary provenance classifier.
//!
//! One binary, six subcommands:
//!
//! - `extract <BIN>`: print the feature JSON for one binary
//! - `classify <BIN>`: predict provenance labels with a trained model
//! - `explain <BIN>`: show which features drive each prediction
//! - `compare <A> <B>`: cosine similarity between two feature vectors
//! - `dataset-info`: label distributions from a manifest CSV
//! - `batch <MANIFEST>`: extract features for every listed binary

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use compid::{elfinfo, features, manifest, model, report};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "compid",
    version,
    about = "Binary provenance classifier - which compiler built this binary?",
    long_about = "compid fingerprints (possibly stripped) ELF binaries and predicts \
                  provenance labels - compiler family, optimization level, architecture, \
                  OS, libc, PIE, LTO, linkage - with confidence scores. Predictions come \
                  from a model the training pipeline produces; the extractor also works \
                  standalone to dump raw features."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print (or write) the feature JSON for one binary
    Extract {
        /// Path to the ELF binary
        bin: PathBuf,
        /// Write the JSON to this file instead of stdout
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Predict provenance labels with a trained model
    Classify {
        /// Path to the ELF binary
        bin: PathBuf,
        /// Trained model file
        #[arg(long, value_name = "FILE", default_value = "model/model.json")]
        model: PathBuf,
        /// Emit machine-readable JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Show which features drive each predicted label
    Explain {
        /// Path to the ELF binary
        bin: PathBuf,
        /// Trained model file
        #[arg(long, value_name = "FILE", default_value = "model/model.json")]
        model: PathBuf,
        /// Emit machine-readable JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Cosine similarity between two binaries' feature vectors
    Compare {
        /// First ELF binary
        a: PathBuf,
        /// Second ELF binary
        b: PathBuf,
    },
    /// Summarize label distributions from a manifest CSV
    DatasetInfo {
        /// Manifest CSV to summarize
        #[arg(long, value_name = "FILE", default_value = "dataset/manifest.csv")]
        manifest: PathBuf,
    },
    /// Extract features for every binary listed in a manifest (JSONL output)
    Batch {
        /// Manifest CSV listing binaries (build_id + path columns required)
        manifest: PathBuf,
        /// Write JSONL here instead of stdout
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli.cmd) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Extract { bin, out } => cmd_extract(&bin, out.as_deref()),
        Cmd::Classify { bin, model, json } => cmd_classify(&bin, &model, json),
        Cmd::Explain { bin, model, json } => cmd_explain(&bin, &model, json),
        Cmd::Compare { a, b } => cmd_compare(&a, &b),
        Cmd::DatasetInfo { manifest } => cmd_dataset_info(&manifest),
        Cmd::Batch { manifest, out } => cmd_batch(&manifest, out.as_deref()),
    }
}

/// Parse + extract in one step, with the binary path in every error.
fn extract_binary(bin: &Path) -> Result<features::ExtractedFeatures> {
    let info = elfinfo::load(bin)?;
    features::extract(&info)
}

fn cmd_extract(bin: &Path, out: Option<&Path>) -> Result<()> {
    let ef = extract_binary(bin)?;
    let json = serde_json::to_string_pretty(&ef).context("feature JSON serialization failed")?;
    match out {
        Some(path) => {
            std::fs::write(path, json + "\n")
                .with_context(|| format!("cannot write {}", path.display()))?;
            eprintln!(
                "wrote {} features for {} (build_id {})",
                ef.features.len(),
                bin.display(),
                ef.build_id
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

fn load_model(path: &Path) -> Result<model::Model> {
    model::Model::load(path)
}

fn cmd_classify(bin: &Path, model_path: &Path, json: bool) -> Result<()> {
    let m = load_model(model_path)?;
    let ef = extract_binary(bin)?;
    let pred = m.predict(&ef.features);
    let text = report::classification(
        bin,
        model_path,
        &m.created,
        m.features.len(),
        &ef,
        &pred,
        json,
    );
    println!("{text}");
    Ok(())
}

fn cmd_explain(bin: &Path, model_path: &Path, json: bool) -> Result<()> {
    let m = load_model(model_path)?;
    let ef = extract_binary(bin)?;
    let pred = m.predict(&ef.features);
    let text = report::explain(bin, &pred, json);
    println!("{text}");
    Ok(())
}

fn cmd_compare(a: &Path, b: &Path) -> Result<()> {
    let fa = extract_binary(a)?;
    let fb = extract_binary(b)?;
    let sim = features::cosine(&fa.features, &fb.features);
    println!("cosine similarity: {sim:.4}");
    println!("{}", report::interpret_similarity(sim));
    Ok(())
}

fn cmd_dataset_info(manifest_path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read manifest {}", manifest_path.display()))?;
    let m = manifest::parse(&text)?;

    println!("manifest: {}", manifest_path.display());
    println!("rows: {}", m.rows.len());
    let mut builds = m
        .rows
        .iter()
        .map(|r| r.build_id.as_str())
        .collect::<Vec<_>>();
    builds.sort_unstable();
    builds.dedup();
    println!("unique builds: {}", builds.len());

    for field in manifest::LABEL_COLUMNS {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for row in &m.rows {
            let value = match *field {
                "compiler_family" => row.compiler_family.as_str(),
                "compiler_version" => row.compiler_version.as_str(),
                "opt_level" => row.opt_level.as_str(),
                "arch" => row.arch.as_str(),
                "os" => row.os.as_str(),
                "abi" => row.abi.as_str(),
                "pie" => row.pie.as_str(),
                "lto" => row.lto.as_str(),
                "static" => row.static_linking.as_str(),
                _ => unreachable!("LABEL_COLUMNS are fixed"),
            };
            *counts.entry(value).or_insert(0) += 1;
        }
        println!();
        println!("{field}");
        for (value, count) in &counts {
            println!("  {value:<24} {count}");
        }
    }
    Ok(())
}

fn cmd_batch(manifest_path: &Path, out: Option<&Path>) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read manifest {}", manifest_path.display()))?;
    let m = manifest::parse(&text)?;
    let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let mut writer: Box<dyn Write> = match out {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path)
                .with_context(|| format!("cannot create {}", path.display()))?,
        )),
        None => Box::new(std::io::BufWriter::new(std::io::stdout().lock())),
    };

    let mut done = 0usize;
    let mut skipped_missing = 0usize;
    let mut skipped_errors = 0usize;
    for row in &m.rows {
        let path = resolve_bin_path(&row.path, base);
        if !path.exists() {
            skipped_missing += 1;
            continue;
        }
        match extract_binary(&path) {
            Ok(mut ef) => {
                // The manifest is ground truth: its build_id and labels
                // replace whatever the extractor inferred on its own.
                ef.build_id = row.build_id.clone();
                ef.labels = features::Labels {
                    compiler_family: row.compiler_family.clone(),
                    compiler_version: row.compiler_version.clone(),
                    opt_level: row.opt_level.clone(),
                    arch: row.arch.clone(),
                    os: row.os.clone(),
                    abi: row.abi.clone(),
                    pie: row.pie.clone(),
                    lto: row.lto.clone(),
                    static_linking: row.static_linking.clone(),
                };
                let line =
                    serde_json::to_string(&ef).context("feature JSON serialization failed")?;
                writeln!(writer, "{line}").context("write failed")?;
                done += 1;
            }
            Err(e) => {
                eprintln!("skip {}: {e}", row.path);
                skipped_errors += 1;
            }
        }
    }
    if writer.flush().is_err() {
        bail!("write failed");
    }

    let target = out.map_or_else(|| "stdout".to_string(), |p| p.display().to_string());
    eprintln!(
        "batch: extracted {done} -> {target}; skipped {skipped_missing} missing, {skipped_errors} failed, of {} rows",
        m.rows.len()
    );
    Ok(())
}

/// Manifest paths are written relative to the repo root; also try them
/// relative to the manifest's own directory so the CSV travels with its
/// binaries.
fn resolve_bin_path(path: &str, base: &Path) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() || p.exists() {
        return p.to_path_buf();
    }
    base.join(p)
}
