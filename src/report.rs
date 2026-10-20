//! Human-readable report rendering for classify, explain, functions, and
//! the one-line interpretation for compare, plus the one-screen `report`
//! combination. Every renderer takes a [`crate::term::Painter`]: plain mode
//! reproduces the historical byte-for-byte output, colored mode adds the
//! lavender/green/amber palette without touching any column geometry.

use crate::features::ExtractedFeatures;
use crate::model::Prediction;
use crate::term::Painter;
use std::collections::BTreeMap;

/// Render a full classification report (human table or JSON).
#[allow(clippy::too_many_arguments)]
pub fn classification(
    bin: &std::path::Path,
    model_ref: &str,
    model_header: &str,
    ef: &ExtractedFeatures,
    pred: &Prediction,
    json: bool,
    confidence_threshold: Option<f64>,
    paint: &Painter,
) -> String {
    if json {
        // Mirror the table's low-confidence flag into the JSON so both sinks
        // agree; probabilities were already there, the flag is the verdict.
        if let Some(t) = confidence_threshold {
            if let Ok(mut value) = serde_json::to_value(pred) {
                if let Some(fields) = value.get_mut("fields").and_then(|f| f.as_array_mut()) {
                    for f in fields {
                        let low = f
                            .get("top3")
                            .and_then(|t3| t3.get(0))
                            .and_then(|top| top.get("probability"))
                            .and_then(|p| p.as_f64())
                            .map(|p| p < t)
                            .unwrap_or(false);
                        if let Some(obj) = f.as_object_mut() {
                            obj.insert("low_confidence".into(), serde_json::Value::Bool(low));
                        }
                    }
                }
                return serde_json::to_string_pretty(&value)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
            }
        }
        return serde_json::to_string_pretty(pred)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    }

    let mut out = String::new();
    out.push_str(&format!("{}{}\n", paint.bold("binary: "), bin.display()));
    out.push_str(&format!(
        "{}{}\n",
        paint.bold("model:   "),
        paint.dim(&format!("{} ({})", model_ref, model_header)),
    ));
    if ef.build_id != "none" {
        out.push_str(&paint.dim(&format!("build-id: {}\n", ef.build_id)));
    }
    for f in &pred.fields {
        out.push('\n');
        let low = confidence_threshold
            .map(|t| f.top3.first().map(|r| r.probability < t).unwrap_or(false))
            .unwrap_or(false);
        out.push_str(&field_block(f, low, paint));
    }
    out
}

/// One field's top-3 table block. Column widths are computed on the raw
/// strings and the color codes are wrapped around the already-padded cells,
/// so colored and plain lines take the same geometry.
fn field_block(f: &crate::model::FieldPrediction, low_confidence: bool, paint: &Painter) -> String {
    let mut out = String::new();
    if low_confidence {
        out.push_str(&format!(
            "{}  {}\n",
            paint.bold(&f.field),
            paint.warn("[low confidence]")
        ));
    } else {
        out.push_str(&paint.bold(&f.field));
        out.push('\n');
    }
    let width = f
        .top3
        .iter()
        .map(|r| r.class.len())
        .max()
        .unwrap_or(0)
        .max(f.predicted.len())
        .max(4);
    for (rank, r) in f.top3.iter().enumerate() {
        let pct = r.probability * 100.0;
        let bar_len = (r.probability * 20.0).round() as usize;
        // Pad first, color second: the escape codes must never count toward
        // the column width.
        let class = format!("{:<width$}", r.class, width = width);
        let class = if r.class == f.predicted {
            paint.good(&class)
        } else {
            class
        };
        out.push_str(&format!(
            "  {rank}. {} {}  {}\n",
            class,
            paint.dim(&format!("{pct:6.2}%")),
            paint.bar(bar_len),
        ));
    }
    out
}

/// Render explain output: for every field, the winning class and the
/// features that pushed it there.
pub fn explain(bin: &std::path::Path, pred: &Prediction, json: bool, paint: &Painter) -> String {
    if json {
        return serde_json::to_string_pretty(pred)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    }
    let mut out = String::new();
    out.push_str(&format!("{}{}\n\n", paint.bold("binary: "), bin.display()));
    for f in &pred.fields {
        out.push_str(&explain_header(f, paint));
        if f.contributions.is_empty() {
            out.push_str(
                &paint.dim("  (no positive contributions; prediction driven by the intercept)\n"),
            );
            continue;
        }
        out.push_str(&paint.dim("  top contributing features:\n"));
        let width = f
            .contributions
            .iter()
            .map(|c| c.feature.len())
            .max()
            .unwrap_or(0);
        for c in &f.contributions {
            out.push_str(&format!(
                "    {}  {sign}{value:.4}  {}\n",
                paint.accent(&format!(
                    "{feature:<width$}",
                    feature = c.feature,
                    width = width
                )),
                paint.dim(&format!("(tf={})", c.tf)),
                sign = if c.contribution >= 0.0 { "+" } else { "" },
                value = c.contribution,
            ));
        }
        out.push('\n');
    }
    out
}

/// The first line of an explain block (or excerpt): field, winning class,
/// confidence.
fn explain_header(f: &crate::model::FieldPrediction, paint: &Painter) -> String {
    let top_pct = f.top3.first().map(|r| r.probability * 100.0).unwrap_or(0.0);
    format!(
        "{}: {} {}\n",
        paint.bold(&f.field),
        paint.good(&f.predicted),
        paint.dim(&format!("({top_pct:.1}% confidence)")),
    )
}

/// A two-line explain excerpt for one field: the winning line and the top
/// contributors on one comma-separated line. Used by the `report` screen,
/// which shows only the most confident fields; with no positive
/// contributions the second line says the intercept carried it.
fn explain_excerpt(f: &crate::model::FieldPrediction, paint: &Painter) -> String {
    let mut out = explain_header(f, paint);
    if f.contributions.is_empty() {
        out.push_str(&paint.dim("  (prediction driven by the intercept)\n"));
        return out;
    }
    let contributors = f
        .contributions
        .iter()
        .take(3)
        .map(|c| {
            format!(
                "{} {sign}{value:.4}",
                paint.accent(&c.feature),
                sign = if c.contribution >= 0.0 { "+" } else { "" },
                value = c.contribution,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("  {contributors}\n"));
    out
}

/// The number of fields the `report` screen excerpts contributors for.
pub const EXCERPT_FIELDS: usize = 4;

/// The one-screen `report` rendering: the classification table (which also
/// draws the binary/model header), then contributor excerpts for the most
/// confident fields, then the model's provenance block. JSON mode emits a
/// single object combining the classification with the provenance.
#[allow(clippy::too_many_arguments)]
pub fn report_screen(
    bin: &std::path::Path,
    model_ref: &str,
    model_header: &str,
    provenance: Option<&crate::model::Provenance>,
    ef: &ExtractedFeatures,
    pred: &Prediction,
    confidence_threshold: Option<f64>,
    json: bool,
    paint: &Painter,
) -> String {
    if json {
        // Reuse the classification JSON verbatim (including the
        // low-confidence flags) rather than re-deriving it.
        let classification_json = classification(
            bin,
            model_ref,
            model_header,
            ef,
            pred,
            true,
            confidence_threshold,
            paint,
        );
        let value: serde_json::Value = serde_json::from_str(&classification_json)
            .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }));
        let obj = serde_json::json!({
            "binary": bin.display().to_string(),
            "model": model_ref,
            "model_header": model_header,
            "build_id": ef.build_id,
            "classification": value,
            "provenance": provenance,
        });
        return serde_json::to_string_pretty(&obj)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    }

    let mut out = classification(
        bin,
        model_ref,
        model_header,
        ef,
        pred,
        false,
        confidence_threshold,
        paint,
    );

    // Most confident first; the stable sort keeps the model's field order
    // for ties so the screen is deterministic.
    let mut ranked: Vec<&crate::model::FieldPrediction> = pred.fields.iter().collect();
    ranked.sort_by(|a, b| {
        let pa = a.top3.first().map(|r| r.probability).unwrap_or(0.0);
        let pb = b.top3.first().map(|r| r.probability).unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
    });
    if !ranked.is_empty() {
        out.push('\n');
        out.push_str(&format!(
            "{}{}\n",
            paint.bold("why"),
            paint.dim(" - top contributors for the most confident fields"),
        ));
        for f in ranked.into_iter().take(EXCERPT_FIELDS) {
            out.push_str(&explain_excerpt(f, paint));
        }
    }

    out.push('\n');
    out.push_str(&provenance_block(provenance, paint));
    out
}

/// The model provenance block for the `report` screen: row counts and
/// sha256 digests in short display form. Full digests stay in the JSON.
fn provenance_block(provenance: Option<&crate::model::Provenance>, paint: &Painter) -> String {
    let mut out = String::new();
    let p = match provenance {
        Some(p) => p,
        None => {
            out.push_str(&paint.dim("provenance: none (model predates the provenance block)\n"));
            return out;
        }
    };
    out.push_str(&paint.bold("provenance"));
    out.push('\n');
    // Labels are padded to a fixed width so the values line up; the dim
    // wrapper goes around the whole row, never inside the geometry.
    let mut row = |label: &str, value: String| {
        out.push_str(&paint.dim(&format!("  {label:<16} {value}\n")));
    };
    if let Some(rows) = p.manifest_rows {
        row("manifest rows:", rows.to_string());
    }
    if let Some(sha) = &p.manifest_sha256 {
        row("manifest sha256:", short_sha(sha));
    }
    if let Some(rows) = p.features_rows {
        row("feature rows:", rows.to_string());
    }
    if let Some(sha) = &p.features_sha256 {
        row("features sha256:", short_sha(sha));
    }
    out
}

/// Short display form of a sha256 digest: the first 12 hex characters plus
/// an ellipsis. The JSON outputs keep the full digest.
pub fn short_sha(sha: &str) -> String {
    let head: String = sha.chars().take(12).collect();
    format!("{head}\u{2026}")
}

/// Plain-language reading of a cosine similarity.
pub fn interpret_similarity(sim: f64) -> &'static str {
    if sim >= 0.99 {
        "near-identical feature vectors; these binaries look like the same code built the same way"
    } else if sim >= 0.90 {
        "very similar; likely the same toolchain and optimization settings"
    } else if sim >= 0.70 {
        "similar; plausibly the same compiler family with different settings"
    } else if sim >= 0.40 {
        "quite different; different toolchains, settings, or mostly different code"
    } else {
        "very different; these binaries share almost no code shape"
    }
}

/// One function's row in the `compid functions` table. `family` and
/// `opt_level` carry the predicted class and its confidence when a
/// function-level model is loaded, and are `None` for a plain listing.
#[derive(Debug, Clone)]
pub struct FunctionRow {
    pub name: String,
    /// Preformatted hexadecimal address (as the symbol table records it).
    pub addr: String,
    pub size: u64,
    pub family: Option<(String, f64)>,
    pub opt_level: Option<(String, f64)>,
}

/// Escaped, truncated display form of a raw symbol name - shared by the
/// functions table and the functions-batch listing. Symbol names come from
/// the binary under analysis; control bytes (newlines, ANSI escapes, other
/// C0) are escaped as \xNN and the result is capped to 48 chars (counted in
/// chars, not bytes, so multi-byte names stay valid UTF-8) with an ASCII
/// ellipsis. The JSON outputs keep the full, unsanitized name (serde handles
/// its own escaping).
pub fn display_name(name: &str) -> String {
    const NAME_CAP: usize = 48;
    let mut escaped = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_control() {
            escaped.push_str(&format!("\\x{:02x}", u32::from(c)));
        } else {
            escaped.push(c);
        }
    }
    if escaped.chars().count() > NAME_CAP {
        let keep: String = escaped.chars().take(NAME_CAP - 3).collect();
        format!("{keep}...")
    } else {
        escaped
    }
}

/// Render the per-function provenance table. When `model` is `Some`, rows
/// carry prediction columns and the footer counts predicted families; when
/// `reason` is `Some` there is nothing to list and the reason stands alone.
/// `source` says where the boundaries came from; the heuristic one is
/// called out in the header so synthesized names are never mistaken for
/// symbol-table names.
#[allow(clippy::too_many_arguments)]
pub fn function_provenance(
    bin: &std::path::Path,
    build_id: &str,
    reason: Option<&str>,
    model: Option<(&std::path::Path, &str)>,
    source: crate::functions::FunctionSource,
    rows: &[FunctionRow],
    paint: &Painter,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}{}\n", paint.bold("binary: "), bin.display()));
    if build_id != "none" {
        out.push_str(&paint.dim(&format!("build-id: {build_id}\n")));
    }
    if source == crate::functions::FunctionSource::Heuristic {
        out.push_str(&paint.warn("function boundaries: prologue heuristic (stripped binary)\n"));
    }
    if let Some((path, header)) = model {
        out.push_str(&format!(
            "{}{}\n",
            paint.bold("model:   "),
            paint.dim(&format!("{} ({})", path.display(), header)),
        ));
    }
    if rows.is_empty() {
        out.push_str(&match reason {
            Some(r) => paint.dim(&format!("\nno functions: {r}\n")),
            None => paint.dim("\nno functions\n"),
        });
        return out;
    }

    let predicted = rows.iter().any(|r| r.family.is_some());
    // Raw symbol-table names can run hundreds of characters (mangled Zig or
    // C++ template names); the table truncates for display, the JSON output
    // never does.
    let display: Vec<String> = rows.iter().map(|r| display_name(&r.name)).collect();
    // Width counts chars to match display_name's char-based truncation
    // (escaped control bytes are ASCII, so byte and char counts agree for
    // them, but multi-byte names would otherwise misalign the table).
    let name_w = display
        .iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    let addr_w = rows.iter().map(|r| r.addr.len()).max().unwrap_or(0).max(3);
    let size_w = rows
        .iter()
        .map(|r| r.size.to_string().len())
        .max()
        .unwrap_or(0)
        .max(4);

    out.push('\n');
    if predicted {
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}  compiler_family        opt_level\n",
            "name", "addr", "size",
        ));
    } else {
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}\n",
            "name", "addr", "size",
        ));
    }
    for (i, r) in rows.iter().enumerate() {
        if !predicted {
            out.push_str(&format!(
                "{:<name_w$}  {:>addr_w$}  {:>size_w$}\n",
                display[i], r.addr, r.size,
            ));
            continue;
        }
        // Each prediction column renders independently: a model may carry
        // only one of the two fields. Cells are padded first and colored
        // second so the codes never shift the columns; the green family is
        // the winning class, the lavender opt level the secondary read.
        let cell = |pred: &Option<(String, f64)>, paint_cell: &dyn Fn(&str) -> String| match pred {
            Some((c, p)) => paint_cell(&format!("{:<12} {:<5.2}", c, p)),
            None => " ".repeat(18),
        };
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}  {}  {}\n",
            display[i],
            r.addr,
            r.size,
            cell(&r.family, &|s| paint.good(s)),
            cell(&r.opt_level, &|s| paint.accent(s)),
        ));
    }

    if predicted {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut with_family = 0usize;
        for r in rows {
            if let Some((f, _)) = &r.family {
                *counts.entry(f.as_str()).or_insert(0) += 1;
                with_family += 1;
            }
        }
        let tally = counts
            .iter()
            .map(|(f, n)| format!("{f}={n}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&paint.dim(&format!(
            "\n{} functions; predicted families: {tally}\n",
            rows.len()
        )));
        if with_family != rows.len() {
            out.push_str(&paint.dim(&format!(
                "note: {} of {} functions carry no compiler_family prediction\n",
                rows.len() - with_family,
                rows.len()
            )));
        }
    } else {
        out.push_str(&paint.dim(&format!("\n{} functions\n", rows.len())));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn similarity_words_match_the_scale() {
        assert!(interpret_similarity(1.0).contains("near-identical"));
        assert!(interpret_similarity(0.95).contains("very similar"));
        assert!(interpret_similarity(0.75).contains("similar"));
        assert!(interpret_similarity(0.5).contains("quite different"));
        assert!(interpret_similarity(0.1).contains("very different"));
    }

    fn demo_rows() -> Vec<FunctionRow> {
        vec![
            FunctionRow {
                name: "handle".to_string(),
                addr: "0x401160".to_string(),
                size: 148,
                family: Some(("gcc".to_string(), 0.97)),
                opt_level: Some(("O2".to_string(), 0.61)),
            },
            FunctionRow {
                name: "emit".to_string(),
                addr: "0x401200".to_string(),
                size: 96,
                family: Some(("clang".to_string(), 0.88)),
                opt_level: Some(("O2".to_string(), 0.55)),
            },
        ]
    }

    #[test]
    fn function_table_with_predictions_has_tally_footer() {
        let text = function_provenance(
            Path::new("mixed-demo"),
            "d1294f",
            None,
            Some((
                Path::new("model/functions.json"),
                "trained 2026-10-20, 3121 features",
            )),
            crate::functions::FunctionSource::SymbolTable,
            &demo_rows(),
            &Painter::plain(),
        );
        assert!(text.contains("binary: mixed-demo"));
        assert!(text.contains("build-id: d1294f"));
        assert!(text.contains("model:   model/functions.json"));
        assert!(text.contains("handle"));
        assert!(text.contains("gcc"));
        assert!(text.contains("clang"));
        assert!(text.contains("2 functions; predicted families: clang=1, gcc=1"));
    }

    #[test]
    fn function_table_without_model_lists_plainly() {
        let rows: Vec<FunctionRow> = demo_rows()
            .into_iter()
            .map(|mut r| {
                r.family = None;
                r.opt_level = None;
                r
            })
            .collect();
        let text = function_provenance(
            Path::new("b"),
            "none",
            None,
            None,
            crate::functions::FunctionSource::SymbolTable,
            &rows,
            &Painter::plain(),
        );
        assert!(!text.contains("compiler_family"));
        assert!(!text.contains("model:"));
        assert!(text.contains("2 functions\n"));
        assert!(!text.contains("predicted families"));
    }

    #[test]
    fn function_table_reports_the_reason_when_empty() {
        let text = function_provenance(
            Path::new("stripped"),
            "none",
            Some("no symbol table: binary looks stripped"),
            None,
            crate::functions::FunctionSource::SymbolTable,
            &[],
            &Painter::plain(),
        );
        assert!(text.contains("no functions: no symbol table: binary looks stripped"));
    }

    #[test]
    fn function_table_notes_the_heuristic_boundaries() {
        let text = function_provenance(
            Path::new("stripped-demo"),
            "none",
            None,
            None,
            crate::functions::FunctionSource::Heuristic,
            &demo_rows(),
            &Painter::plain(),
        );
        assert!(text.contains("function boundaries: prologue heuristic (stripped binary)"));
        // The symbol-table rendering stays clean of the note.
        let plain = function_provenance(
            Path::new("stripped-demo"),
            "none",
            None,
            None,
            crate::functions::FunctionSource::SymbolTable,
            &demo_rows(),
            &Painter::plain(),
        );
        assert!(!plain.contains("prologue heuristic"));
    }
}

#[cfg(test)]
mod escape_tests {
    use super::display_name;

    #[test]
    fn control_bytes_are_escaped_for_terminal_sinks() {
        assert_eq!(display_name("ok_name"), "ok_name");
        assert_eq!(display_name("ev\x1b[31mil"), "ev\\x1b[31mil");
        assert_eq!(display_name("in\njection\r"), "in\\x0ajection\\x0d");
    }

    #[test]
    fn truncation_still_applies_after_escaping() {
        let long = format!("{}{}", "a", "\t".repeat(60));
        let out = display_name(&long);
        assert!(out.chars().count() <= 48);
        assert!(out.contains("\\x09"));
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;
    use crate::features::Labels;
    use crate::model::{Contribution, FieldPrediction, RankedClass};
    use crate::term::{AMBER, BOLD, DIM, GREEN, LAVENDER, RESET};
    use std::path::Path;

    fn labels() -> Labels {
        Labels {
            compiler_family: String::new(),
            compiler_version: String::new(),
            opt_level: String::new(),
            arch: String::new(),
            os: String::new(),
            abi: String::new(),
            pie: String::new(),
            lto: String::new(),
            static_linking: String::new(),
            hardening: String::new(),
        }
    }

    fn demo_features() -> ExtractedFeatures {
        ExtractedFeatures {
            build_id: "d1294f531713bb38".to_string(),
            labels: labels(),
            features: std::collections::BTreeMap::new(),
        }
    }

    fn demo_prediction() -> Prediction {
        Prediction {
            fields: vec![
                FieldPrediction {
                    field: "compiler_family".to_string(),
                    predicted: "gcc".to_string(),
                    top3: vec![
                        RankedClass {
                            class: "gcc".to_string(),
                            probability: 0.9988,
                        },
                        RankedClass {
                            class: "clang".to_string(),
                            probability: 0.0012,
                        },
                    ],
                    contributions: vec![Contribution {
                        feature: "mn:mov".to_string(),
                        tf: 17,
                        contribution: 2.161,
                    }],
                },
                FieldPrediction {
                    field: "opt_level".to_string(),
                    predicted: "O1".to_string(),
                    top3: vec![
                        RankedClass {
                            class: "O1".to_string(),
                            probability: 0.4158,
                        },
                        RankedClass {
                            class: "Os".to_string(),
                            probability: 0.3333,
                        },
                        RankedClass {
                            class: "O3".to_string(),
                            probability: 0.1922,
                        },
                    ],
                    contributions: Vec::new(),
                },
            ],
        }
    }

    /// Plain mode is pinned byte-for-byte: any change here is a visible
    /// change to the historical output and must be made deliberately.
    #[test]
    fn classification_plain_output_is_byte_stable() {
        let text = classification(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            &demo_features(),
            &demo_prediction(),
            false,
            Some(0.6),
            &Painter::plain(),
        );
        #[rustfmt::skip]
        let expected = concat!(
            "binary: /tmp/sample\n",
            "model:   model/model.json (trained 2026-10-20, 68352 features)\n",
            "build-id: d1294f531713bb38\n",
            "\n",
            "compiler_family\n",
            "  0. gcc    99.88%  ####################\n",
            "  1. clang   0.12%  \n",
            "\n",
            "opt_level  [low confidence]\n",
            "  0. O1    41.58%  ########\n",
            "  1. Os    33.33%  #######\n",
            "  2. O3    19.22%  ####\n",
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn classification_colored_output_carries_the_palette() {
        let text = classification(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            &demo_features(),
            &demo_prediction(),
            false,
            Some(0.6),
            &Painter::colored(),
        );
        assert!(text.contains(&format!("{BOLD}binary: {RESET}")));
        assert!(text.contains(&format!(
            "{DIM}model/model.json (trained 2026-10-20, 68352 features){RESET}"
        )));
        assert!(text.contains(&format!("{BOLD}compiler_family{RESET}")));
        // The winning class is green, the runner-up stays bare, and the
        // padding happens before the codes so the column still lines up.
        assert!(text.contains(&format!("  0. {GREEN}gcc  {RESET}")));
        assert!(text.contains("  1. clang "));
        assert!(text.contains(&format!("{DIM} 99.88%{RESET}")));
        assert!(text.contains(&format!("{LAVENDER}{}{RESET}", "\u{2588}".repeat(20))));
        assert!(text.contains(&format!(
            "{BOLD}opt_level{RESET}  {AMBER}[low confidence]{RESET}"
        )));
        // Colored output keeps the same visible line structure: strip the
        // codes and fold the block glyph back to its plain twin, and the
        // pinned plain text must fall out.
        let stripped = strip_ansi(&text).replace('\u{2588}', "#");
        let plain = classification(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            &demo_features(),
            &demo_prediction(),
            false,
            Some(0.6),
            &Painter::plain(),
        );
        assert_eq!(stripped, plain);
    }

    /// Remove SGR sequences so a test can compare colored rendering against
    /// the plain rendering by visible text alone.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for e in chars.by_ref() {
                    if e == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn explain_plain_output_is_byte_stable() {
        let text = explain(
            Path::new("/tmp/sample"),
            &demo_prediction(),
            false,
            &Painter::plain(),
        );
        #[rustfmt::skip]
        let expected = concat!(
            "binary: /tmp/sample\n",
            "\n",
            "compiler_family: gcc (99.9% confidence)\n",
            "  top contributing features:\n",
            "    mn:mov  +2.1610  (tf=17)\n",
            "\n",
            "opt_level: O1 (41.6% confidence)\n",
            "  (no positive contributions; prediction driven by the intercept)\n",
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn report_screen_excerpts_the_most_confident_fields() {
        let provenance = crate::model::Provenance {
            manifest_rows: Some(1389),
            manifest_sha256: Some(
                "b2568ab4d279bcd56df7e8d16b1eda5019c6e48263c0ac4c29bac00c5d47eb9c".to_string(),
            ),
            features_rows: Some(1389),
            features_sha256: None,
        };
        let text = report_screen(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            Some(&provenance),
            &demo_features(),
            &demo_prediction(),
            Some(0.6),
            false,
            &Painter::plain(),
        );
        // Classification table first, excerpts second, provenance last.
        assert!(text.contains("compiler_family\n"));
        assert!(text.contains("why - top contributors for the most confident fields\n"));
        assert!(text.contains("compiler_family: gcc (99.9% confidence)\n"));
        assert!(text.contains("  mn:mov +2.1610\n"));
        // opt_level is less confident and lands after compiler_family; only
        // the top four fields are excerpted.
        let excerpt_pos = text
            .find("why - top contributors")
            .expect("excerpt heading");
        let opt_pos = text.find("opt_level: O1").expect("opt_level excerpt");
        assert!(excerpt_pos < opt_pos);
        assert!(text.contains("provenance\n"));
        assert!(text.contains("  manifest rows:   1389\n"));
        assert!(text.contains("  manifest sha256: b2568ab4d279\u{2026}\n"));
        assert!(!text.contains("features sha256"));
    }

    #[test]
    fn report_screen_colored_marks_the_sections() {
        let text = report_screen(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            None,
            &demo_features(),
            &demo_prediction(),
            Some(0.6),
            false,
            &Painter::colored(),
        );
        assert!(text.contains(&format!("{BOLD}why{RESET}")));
        // No provenance block on this model: the None case renders as a
        // dim note instead of a bold section.
        assert!(text.contains(&format!(
            "{DIM}provenance: none (model predates the provenance block)\n{RESET}"
        )));
        assert!(text.contains(LAVENDER));
    }

    #[test]
    fn report_screen_json_combines_classification_and_provenance() {
        let provenance = crate::model::Provenance {
            manifest_rows: Some(7),
            manifest_sha256: Some("abcdef1234567890".to_string()),
            features_rows: None,
            features_sha256: None,
        };
        let text = report_screen(
            Path::new("/tmp/sample"),
            "model/model.json",
            "trained 2026-10-20, 68352 features",
            Some(&provenance),
            &demo_features(),
            &demo_prediction(),
            Some(0.6),
            true,
            &Painter::plain(),
        );
        assert!(!text.contains('\x1b'), "JSON stays plain");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["binary"], "/tmp/sample");
        assert_eq!(value["model_header"], "trained 2026-10-20, 68352 features");
        assert_eq!(value["build_id"], "d1294f531713bb38");
        assert_eq!(
            value["classification"]["fields"][0]["field"],
            "compiler_family"
        );
        assert_eq!(
            value["classification"]["fields"][0]["low_confidence"],
            false
        );
        assert_eq!(value["classification"]["fields"][1]["low_confidence"], true);
        assert_eq!(value["provenance"]["manifest_rows"], 7);
        assert_eq!(value["provenance"]["manifest_sha256"], "abcdef1234567890");
    }

    #[test]
    fn short_sha_truncates_to_twelve_chars_and_an_ellipsis() {
        assert_eq!(
            short_sha("b2568ab4d279bcd56df7e8d16b1eda5019c6e48263c0ac4c29bac00c5d47eb9c"),
            "b2568ab4d279\u{2026}"
        );
        assert_eq!(short_sha("abcdef"), "abcdef\u{2026}");
    }

    #[test]
    fn explain_excerpt_is_two_lines_with_three_contributors() {
        let mut f = FieldPrediction {
            field: "abi".to_string(),
            predicted: "gnu".to_string(),
            top3: vec![RankedClass {
                class: "gnu".to_string(),
                probability: 0.9988,
            }],
            contributions: (0..5)
                .map(|i| Contribution {
                    feature: format!("sec:.s{i}"),
                    tf: 8,
                    contribution: 0.27 - i as f64 * 0.01,
                })
                .collect(),
        };
        let text = explain_excerpt(&f, &Painter::plain());
        assert_eq!(
            text,
            "abi: gnu (99.9% confidence)\n  sec:.s0 +0.2700, sec:.s1 +0.2600, sec:.s2 +0.2500\n"
        );
        let colored = explain_excerpt(&f, &Painter::colored());
        assert!(colored.contains(&format!("{BOLD}abi{RESET}")));
        assert!(colored.contains(&format!("{GREEN}gnu{RESET}")));
        assert!(colored.contains(&format!("{LAVENDER}sec:.s0{RESET}")));
        assert_eq!(colored.lines().count(), 2);

        f.contributions.clear();
        let empty = explain_excerpt(&f, &Painter::plain());
        assert_eq!(
            empty,
            "abi: gnu (99.9% confidence)\n  (prediction driven by the intercept)\n"
        );
    }
}
