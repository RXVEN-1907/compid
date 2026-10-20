//! Model loading and inference math.
//!
//! `model.json` (format `compid-model/v1`) is a multi-class linear model per
//! label field. For a feature vector `x` (L2-normalized over the model's
//! named features, missing features = 0):
//!
//! ```text
//! score_c    = intercept[c] + dot(coef[c], x)
//! prob_c     = softmax(scores)_c
//! ```
//!
//! The Python trainer writes the file; this module is the single reader.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;

/// Format tag the reader enforces.
pub const MODEL_FORMAT: &str = "compid-model/v1";
/// Feature schema version the reader enforces.
pub const FEATURE_VERSION: &str = "1";

// Models embedded at build time: a released binary classifies with no files
// beside it. The .gz copies are committed next to the plain files and kept
// deterministic (no timestamp, level 9) by the trainers, which rewrite them
// on every run. An explicit --model always wins over these fallbacks.
static EMBEDDED_MODEL_GZ: &[u8] = include_bytes!("../model/model.json.gz");
static EMBEDDED_FUNCTIONS_GZ: &[u8] = include_bytes!("../model/functions.json.gz");

/// Decompress an embedded gzip model payload into its JSON text.
fn embedded_text(bytes: &[u8], what: &str) -> Result<String> {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .with_context(|| format!("embedded {what} gzip payload is corrupt"))?;
    Ok(text)
}

/// One label field's linear model: classes in fixed order, an intercept and
/// a coefficient row per class.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldModel {
    pub classes: Vec<String>,
    pub intercept: Vec<f64>,
    pub coef: Vec<Vec<f64>>,
}

/// Optional provenance block: ties a model back to the exact dataset it was
/// fitted on, so a user can tell how fresh the model they are running is.
/// Whole models trained before provenance was introduced lack the block;
/// every field is individually optional for the same reason.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Provenance {
    #[serde(default)]
    pub manifest_rows: Option<u64>,
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    #[serde(default)]
    pub features_rows: Option<u64>,
    #[serde(default)]
    pub features_sha256: Option<String>,
}

/// A full trained model.
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub format: String,
    pub feature_version: String,
    pub created: String,
    /// Ordered feature names; coefficient rows are indexed against this.
    pub features: Vec<String>,
    pub fields: BTreeMap<String, FieldModel>,
    #[serde(default)]
    pub provenance: Option<Provenance>,

    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl Model {
    /// One-line summary for report headers: what the model is and, when the
    /// provenance block is present, how much data stands behind it.
    pub fn header_line(&self) -> String {
        match self.provenance.as_ref().and_then(|p| p.manifest_rows) {
            Some(rows) => format!(
                "trained {} over {} builds, {} features",
                self.created,
                rows,
                self.features.len()
            ),
            None => format!("trained {}, {} features", self.created, self.features.len()),
        }
    }

    /// Load and validate a model file from disk.
    pub fn load(path: &Path) -> Result<Model> {
        if !path.exists() {
            bail!(
                "no model file at '{}': train one first (see training/) or pass --model <PATH>",
                path.display()
            );
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read model file {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("invalid model file {}", path.display()))
    }

    /// The whole-binary model embedded at build time (model/model.json.gz),
    /// parsed once per process. This is what runs when no model file exists
    /// beside the caller - the released binaries are self-contained.
    pub fn embedded() -> Result<Model> {
        static TEXT: OnceLock<Result<String, String>> = OnceLock::new();
        let text = TEXT
            .get_or_init(|| {
                embedded_text(EMBEDDED_MODEL_GZ, "model.json").map_err(|e| format!("{e:#}"))
            })
            .as_ref()
            .map_err(|e| anyhow!("{e}"))?;
        Self::parse(text)
    }

    /// The per-function model embedded at build time
    /// (model/functions.json.gz), same contract as [`Model::embedded`].
    pub fn embedded_functions() -> Result<Model> {
        static TEXT: OnceLock<Result<String, String>> = OnceLock::new();
        let text = TEXT
            .get_or_init(|| {
                embedded_text(EMBEDDED_FUNCTIONS_GZ, "functions.json").map_err(|e| format!("{e:#}"))
            })
            .as_ref()
            .map_err(|e| anyhow!("{e}"))?;
        Self::parse(text)
    }

    /// Parse and validate a model from JSON text.
    pub fn parse(text: &str) -> Result<Model> {
        let mut model: Model =
            serde_json::from_str(text).context("model.json is not valid JSON")?;
        if model.format != MODEL_FORMAT {
            bail!(
                "expected format \"{}\", found \"{}\"",
                MODEL_FORMAT,
                model.format
            );
        }
        if model.feature_version != FEATURE_VERSION {
            bail!(
                "expected feature_version \"{}\", found \"{}\"",
                FEATURE_VERSION,
                model.feature_version
            );
        }
        if model.features.is_empty() {
            bail!("model has no features");
        }
        if model.fields.is_empty() {
            bail!("model has no label fields");
        }
        for (i, name) in model.features.iter().enumerate() {
            model.index.insert(name.clone(), i);
        }
        for (field, fm) in &model.fields {
            if fm.classes.is_empty() {
                bail!("field \"{field}\" has no classes");
            }
            if fm.intercept.len() != fm.classes.len() {
                bail!(
                    "field \"{}\": intercept count ({}) does not match class count ({})",
                    field,
                    fm.intercept.len(),
                    fm.classes.len()
                );
            }
            if fm.coef.len() != fm.classes.len() {
                bail!(
                    "field \"{}\": coef row count ({}) does not match class count ({})",
                    field,
                    fm.coef.len(),
                    fm.classes.len()
                );
            }
            for (c, row) in fm.coef.iter().enumerate() {
                if row.len() != model.features.len() {
                    bail!(
                        "field \"{}\" class \"{}\": coef width ({}) does not match feature count ({})",
                        field,
                        fm.classes[c],
                        row.len(),
                        model.features.len()
                    );
                }
            }
        }
        Ok(model)
    }

    /// Dense raw tf vector aligned with `self.features` (missing = 0).
    pub fn vectorize(&self, features: &BTreeMap<String, u64>) -> Vec<f64> {
        self.features
            .iter()
            .map(|name| features.get(name).copied().unwrap_or(0) as f64)
            .collect()
    }

    /// In-place L2 normalization; a zero vector stays zero.
    pub fn normalize_l2(v: &mut [f64]) {
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 && norm.is_finite() {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }

    /// Predict every label field for one feature vector.
    pub fn predict(&self, features: &BTreeMap<String, u64>) -> Prediction {
        let mut x = self.vectorize(features);
        Self::normalize_l2(&mut x);

        let mut fields = Vec::new();
        for (field, fm) in &self.fields {
            let scores: Vec<f64> = fm
                .classes
                .iter()
                .enumerate()
                .map(|(c, _)| fm.intercept[c] + dot(&fm.coef[c], &x))
                .collect();
            let probs = softmax(&scores);

            let mut order: Vec<usize> = (0..probs.len()).collect();
            order.sort_by(|&a, &b| {
                probs[b]
                    .partial_cmp(&probs[a])
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.cmp(&b))
            });

            let top3: Vec<RankedClass> = order
                .iter()
                .take(3)
                .map(|&c| RankedClass {
                    class: fm.classes[c].clone(),
                    probability: probs[c],
                })
                .collect();

            let top_idx = order[0];
            let mut contributions: Vec<(usize, f64)> = (0..self.features.len())
                .map(|i| (i, fm.coef[top_idx][i] * x[i]))
                .filter(|&(_, v)| v > 0.0)
                .collect();
            contributions.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            let contributions: Vec<Contribution> = contributions
                .iter()
                .take(8)
                .map(|&(i, v)| Contribution {
                    feature: self.features[i].clone(),
                    tf: features.get(&self.features[i]).copied().unwrap_or(0),
                    contribution: v,
                })
                .collect();

            fields.push(FieldPrediction {
                field: field.clone(),
                predicted: fm.classes[top_idx].clone(),
                top3,
                contributions,
            });
        }
        Prediction { fields }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Numerically stable softmax; uniform on degenerate (non-finite) input.
pub fn softmax(scores: &[f64]) -> Vec<f64> {
    if scores.is_empty() {
        return Vec::new();
    }
    let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !max.is_finite() {
        return vec![1.0 / scores.len() as f64; scores.len()];
    }
    let exps: Vec<f64> = scores.iter().map(|&s| (s - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    if sum <= 0.0 || !sum.is_finite() {
        return vec![1.0 / scores.len() as f64; scores.len()];
    }
    exps.into_iter().map(|e| e / sum).collect()
}

/// One class with its softmax probability.
#[derive(Debug, Clone, Serialize)]
pub struct RankedClass {
    pub class: String,
    pub probability: f64,
}

/// One feature's contribution to a predicted class: `coef[class][f] * x[f]`.
#[derive(Debug, Clone, Serialize)]
pub struct Contribution {
    pub feature: String,
    /// Raw count of the feature in the analyzed binary.
    pub tf: u64,
    pub contribution: f64,
}

/// Per-field prediction for one binary.
#[derive(Debug, Clone, Serialize)]
pub struct FieldPrediction {
    pub field: String,
    pub predicted: String,
    pub top3: Vec<RankedClass>,
    pub contributions: Vec<Contribution>,
}

/// All per-field predictions for one binary.
#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub fields: Vec<FieldPrediction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two features, one field, hand-computed weights:
    /// gcc = [1, 0] + 0, clang = [0, 1] + 0.
    fn toy_model() -> Model {
        let json = r#"{
            "format": "compid-model/v1",
            "feature_version": "1",
            "created": "2026-10-19",
            "features": ["mn:pop", "mn:push"],
            "fields": {
                "compiler_family": {
                    "classes": ["gcc", "clang"],
                    "intercept": [0.0, 0.0],
                    "coef": [[1.0, 0.0], [0.0, 1.0]]
                }
            }
        }"#;
        Model::parse(json).expect("toy model parses")
    }

    #[test]
    fn parses_and_validates() {
        let m = toy_model();
        assert_eq!(m.features, vec!["mn:pop", "mn:push"]);
        assert!(m.index.contains_key("mn:push"));

        let bad = r#"{
            "format": "compid-model/v2",
            "feature_version": "1",
            "created": "2026-10-19",
            "features": ["a"],
            "fields": {"f": {"classes": ["x"], "intercept": [0.0], "coef": [[1.0]]}}
        }"#;
        assert!(Model::parse(bad).is_err());

        let ragged = r#"{
            "format": "compid-model/v1",
            "feature_version": "1",
            "created": "2026-10-19",
            "features": ["a", "b"],
            "fields": {"f": {"classes": ["x"], "intercept": [0.0], "coef": [[1.0]]}}
        }"#;
        assert!(Model::parse(ragged).is_err());
    }

    #[test]
    fn softmax_matches_hand_computed_values() {
        let p = softmax(&[1.0, 0.0]);
        // e^1 / (e^1 + 1) = 0.7310585786300049
        assert!(
            (p[0] - 0.731_058_578_630_004_9).abs() < 1e-12,
            "p[0] = {}",
            p[0]
        );
        assert!(
            (p[1] - 0.268_941_421_369_995_1).abs() < 1e-12,
            "p[1] = {}",
            p[1]
        );
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        // shift invariance
        let q = softmax(&[101.0, 100.0]);
        assert!((q[0] - p[0]).abs() < 1e-12);
        // degenerate input stays uniform
        let r = softmax(&[f64::INFINITY, 0.0]);
        assert!((r[0] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn l2_normalization_is_exact() {
        let mut v = vec![3.0, 4.0];
        Model::normalize_l2(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-12);
        assert!((v[1] - 0.8).abs() < 1e-12);
        let mut zero = vec![0.0, 0.0];
        Model::normalize_l2(&mut zero);
        assert_eq!(zero, vec![0.0, 0.0]);
    }

    #[test]
    fn predict_argmax_and_probabilities() {
        let m = toy_model();
        let mut feats = BTreeMap::new();
        feats.insert("mn:pop".to_string(), 1u64);
        let pred = m.predict(&feats);
        let f = &pred.fields[0];
        assert_eq!(f.field, "compiler_family");
        assert_eq!(f.predicted, "gcc");
        assert!((f.top3[0].probability - 0.731_058_578_630_004_9).abs() < 1e-12);
        assert_eq!(f.top3[0].class, "gcc");
        assert_eq!(f.top3[1].class, "clang");
        assert_eq!(f.top3.len(), 2); // only two classes exist
                                     // the winning feature explains the win
        assert_eq!(f.contributions[0].feature, "mn:pop");
        assert!((f.contributions[0].contribution - 1.0).abs() < 1e-12);
        assert_eq!(f.contributions[0].tf, 1);
    }

    #[test]
    fn predict_uses_l2_norm_and_missing_features_are_zero() {
        let m = toy_model();
        let mut feats = BTreeMap::new();
        feats.insert("mn:pop".to_string(), 3u64);
        feats.insert("mn:push".to_string(), 4u64);
        // x = [0.6, 0.8]; score(gcc) = 0.6, score(clang) = 0.8 -> clang wins
        let pred = m.predict(&feats);
        assert_eq!(pred.fields[0].predicted, "clang");

        // feature outside the model vocabulary is ignored entirely
        let mut feats = BTreeMap::new();
        feats.insert("mn:ret".to_string(), 100u64);
        let pred = m.predict(&feats);
        let f = &pred.fields[0];
        assert_eq!(f.predicted, "gcc"); // tie broken by class order
        assert!((f.top3[0].probability - 0.5).abs() < 1e-12);
    }
}
