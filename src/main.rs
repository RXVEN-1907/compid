//! compid - binary provenance classifier.
//!
//! One binary, eight subcommands plus a hidden completions helper:
//!
//! - `extract <BIN>`: print the feature JSON for one binary
//! - `classify <BIN>`: predict provenance labels with a trained model
//! - `explain <BIN>`: show which features drive each prediction
//! - `compare <A> <B>`: cosine similarity between two feature vectors
//! - `dataset-info`: label distributions from a manifest CSV
//! - `batch <MANIFEST>`: extract features for every listed binary
//! - `functions <BIN>`: per-function provenance table or training features
//! - `functions-batch <MANIFEST>`: per-function extraction over a manifest
//! - `completions <SHELL>`: print a shell completion script (hidden)

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use compid::{
    extract_binary, extract_binary_functions, features, functions, manifest, model, report,
};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "compid",
    version,
    about = "Binary provenance classifier - which compiler built this binary?",
    long_about = "compid fingerprints (possibly stripped) ELF and PE (x86-64) binaries \
                  and predicts provenance labels - compiler family, optimization level, \
                  architecture, OS, libc, PIE, LTO, linkage - with confidence scores. \
                  Predictions come from a model the training pipeline produces; the \
                  extractor also works standalone to dump raw features."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print (or write) the feature JSON for one binary
    Extract {
        /// Path to an ELF, PE, or Mach-O binary
        bin: PathBuf,
        /// Write the JSON to this file instead of stdout
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Predict provenance labels with a trained model
    Classify {
        /// Path to an ELF, PE, or Mach-O binary
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
        /// Path to an ELF, PE, or Mach-O binary
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
        /// First ELF, PE, or Mach-O binary
        a: PathBuf,
        /// Second ELF, PE, or Mach-O binary
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
    /// Per-function provenance for one binary (unstripped ELF today)
    Functions {
        /// Path to an ELF binary (PE and Mach-O are reported as unsupported)
        bin: PathBuf,
        /// Trained per-function model; omit it for a plain function listing
        #[arg(long, value_name = "FILE")]
        model: Option<PathBuf>,
        /// Write per-function training features (JSONL) here instead of the table
        #[arg(long, value_name = "FILE")]
        features: Option<PathBuf>,
        /// Emit machine-readable JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Per-function extraction over every manifest row with an unstripped ELF binary
    FunctionsBatch {
        /// Manifest CSV listing binaries (build_id + path columns required)
        manifest: PathBuf,
        /// Write output here instead of stdout
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Write per-function training features (JSONL) with manifest labels
        #[arg(long)]
        features: bool,
    },
    /// Print a shell completion script to stdout (hidden from --help)
    #[command(hide = true)]
    Completions {
        /// Shell to generate the completion script for
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

/// The shells `compid completions` writes scripts for, in the order they are
/// offered on the command line.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    /// Windows PowerShell
    #[value(name = "powershell")]
    PowerShell,
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
        Cmd::Functions {
            bin,
            model,
            features,
            json,
        } => cmd_functions(&bin, model.as_deref(), features.as_deref(), json),
        Cmd::FunctionsBatch {
            manifest,
            out,
            features,
        } => cmd_functions_batch(&manifest, out.as_deref(), features),
        Cmd::Completions { shell } => cmd_completions(shell),
    }
}

/// Write the completion script for one shell to stdout. The script covers
/// the subcommands and flags of the build that generated it.
fn cmd_completions(shell: CompletionShell) -> Result<()> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_owned();
    let generator = match shell {
        CompletionShell::Bash => clap_complete::shells::Shell::Bash,
        CompletionShell::Zsh => clap_complete::shells::Shell::Zsh,
        CompletionShell::Fish => clap_complete::shells::Shell::Fish,
        CompletionShell::PowerShell => clap_complete::shells::Shell::PowerShell,
    };
    let mut out = std::io::stdout().lock();
    clap_complete::generate(generator, &mut cmd, name, &mut out);
    out.flush().context("write failed")?;
    Ok(())
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

/// Predict the two function-sensible fields for one function's features and
/// build the display row, including its name, address, and size.
fn predict_function(
    m: &model::Model,
    name: &str,
    addr: &str,
    size: u64,
    feats: &BTreeMap<String, u64>,
) -> report::FunctionRow {
    let pred = m.predict(feats);
    let class_of = |field: &str| {
        pred.fields
            .iter()
            .find(|f| f.field == field)
            .and_then(|f| f.top3.first())
            .map(|r| (r.class.clone(), r.probability))
    };
    report::FunctionRow {
        name: name.to_string(),
        addr: addr.to_string(),
        size,
        family: class_of("compiler_family"),
        opt_level: class_of("opt_level"),
    }
}

fn cmd_functions(
    bin: &Path,
    model_arg: Option<&Path>,
    features_out: Option<&Path>,
    json: bool,
) -> Result<()> {
    let ex = extract_binary_functions(bin)?;

    if let Some(path) = features_out {
        return cmd_functions_features(&ex, path);
    }

    // The default model path is used only when the file exists, so a plain
    // `compid functions <BIN>` still lists functions on a fresh checkout; an
    // explicitly passed --model that is missing stays a hard error.
    let default_model = Path::new("model/functions.json");
    let model_path: Option<&Path> = match model_arg {
        Some(p) => Some(p),
        None if default_model.exists() => Some(default_model),
        None => None,
    };
    let m = match model_path {
        Some(p) => Some(load_model(p)?),
        None => None,
    };

    if json {
        let obj = functions_json(bin, model_path, &ex, m.as_ref())?;
        println!("{obj}");
        return Ok(());
    }

    let rows: Vec<report::FunctionRow> = match &m {
        Some(m) => ex
            .functions
            .iter()
            .map(|f| predict_function(m, &f.name, &format!("0x{:x}", f.addr), f.size, &f.features))
            .collect(),
        None => ex
            .functions
            .iter()
            .map(|f| report::FunctionRow {
                name: f.name.clone(),
                addr: format!("0x{:x}", f.addr),
                size: f.size,
                family: None,
                opt_level: None,
            })
            .collect(),
    };

    let model_line = match (&m, model_path) {
        (Some(m), Some(p)) => Some((p, m.created.as_str(), m.features.len())),
        _ => None,
    };
    let text =
        report::function_provenance(bin, &ex.build_id, ex.reason.as_deref(), model_line, &rows);
    println!("{text}");
    Ok(())
}

/// Per-function training JSONL for one binary: one object per function with
/// the extractor's own best-effort labels (batch mode replaces them with the
/// manifest's ground truth).
fn cmd_functions_features(ex: &functions::FunctionsExtraction, path: &Path) -> Result<()> {
    if ex.functions.is_empty() {
        bail!(
            "no functions to write ({}): pass an unstripped ELF",
            ex.reason.as_deref().unwrap_or("empty function list")
        );
    }
    let file =
        std::fs::File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    for f in &ex.functions {
        let line = serde_json::to_string(&functions::FunctionTrainingRow {
            build_id: &ex.build_id,
            name: &f.name,
            addr: format!("0x{:x}", f.addr),
            size: f.size,
            labels: &ex.labels,
            features: &f.features,
        })
        .context("feature JSON serialization failed")?;
        writeln!(writer, "{line}").context("write failed")?;
    }
    writer.flush().context("write failed")?;
    eprintln!(
        "wrote {} function feature rows for {} (build_id {})",
        ex.functions.len(),
        path.display(),
        ex.build_id
    );
    Ok(())
}

/// Machine-readable form of `compid functions`.
fn functions_json(
    bin: &Path,
    model_path: Option<&Path>,
    ex: &functions::FunctionsExtraction,
    m: Option<&model::Model>,
) -> Result<String> {
    use serde_json::json;

    let mut functions_out = Vec::new();
    let mut family_counts: BTreeMap<String, u64> = BTreeMap::new();
    for f in &ex.functions {
        let (family, opt_level) = match m {
            Some(m) => {
                let fr =
                    predict_function(m, &f.name, &format!("0x{:x}", f.addr), f.size, &f.features);
                if let Some((cls, _)) = &fr.family {
                    *family_counts.entry(cls.clone()).or_insert(0) += 1;
                }
                (
                    fr.family
                        .map(|(c, p)| json!({"predicted": c, "confidence": p})),
                    fr.opt_level
                        .map(|(c, p)| json!({"predicted": c, "confidence": p})),
                )
            }
            None => (None, None),
        };
        functions_out.push(json!({
            "name": f.name,
            "addr": format!("0x{:x}", f.addr),
            "size": f.size,
            "compiler_family": family,
            "opt_level": opt_level,
        }));
    }

    let obj = json!({
        "binary": bin.display().to_string(),
        "model": model_path.map(|p| p.display().to_string()),
        "build_id": ex.build_id,
        "reason": ex.reason,
        "functions": functions_out,
        "family_counts": family_counts,
    });
    serde_json::to_string_pretty(&obj).context("JSON serialization failed")
}

fn cmd_functions_batch(manifest_path: &Path, out: Option<&Path>, features: bool) -> Result<()> {
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

    let mut functions_written = 0usize;
    let mut binaries_done = 0usize;
    let mut skipped_missing = 0usize;
    let mut skipped_no_functions = 0usize;
    let mut skipped_errors = 0usize;
    for row in &m.rows {
        let path = resolve_bin_path(&row.path, base);
        if !path.exists() {
            skipped_missing += 1;
            continue;
        }
        let ex = match extract_binary_functions(&path) {
            Ok(ex) => ex,
            Err(e) => {
                eprintln!("skip {}: {e}", row.path);
                skipped_errors += 1;
                continue;
            }
        };
        if ex.functions.is_empty() {
            eprintln!(
                "skip {}: {}",
                row.path,
                ex.reason.as_deref().unwrap_or("no functions")
            );
            skipped_no_functions += 1;
            continue;
        }

        let labels = features::Labels {
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

        if features {
            for f in &ex.functions {
                let line = serde_json::to_string(&functions::FunctionTrainingRow {
                    build_id: &row.build_id,
                    name: &f.name,
                    addr: format!("0x{:x}", f.addr),
                    size: f.size,
                    labels: &labels,
                    features: &f.features,
                })
                .context("feature JSON serialization failed")?;
                writeln!(writer, "{line}").context("write failed")?;
                functions_written += 1;
            }
        } else {
            writeln!(writer, "== {} (build_id {})", row.path, row.build_id)
                .context("write failed")?;
            for f in &ex.functions {
                writeln!(writer, "{:<40}  0x{:x}  {}", f.name, f.addr, f.size)
                    .context("write failed")?;
            }
            writeln!(writer).context("write failed")?;
        }
        binaries_done += 1;
    }
    writer.flush().ok();

    let target = out.map_or_else(|| "stdout".to_string(), |p| p.display().to_string());
    if features {
        eprintln!(
            "functions-batch: wrote {functions_written} function rows from {binaries_done} binaries -> {target}; \
             skipped {skipped_missing} missing, {skipped_no_functions} without functions, {skipped_errors} failed, \
             of {} rows",
            m.rows.len()
        );
    } else {
        eprintln!(
            "functions-batch: listed {binaries_done} binaries -> {target}; \
             skipped {skipped_missing} missing, {skipped_no_functions} without functions, {skipped_errors} failed, \
             of {} rows",
            m.rows.len()
        );
    }
    Ok(())
}
