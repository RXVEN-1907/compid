//! compid - binary provenance classifier.
//!
//! Given a (possibly stripped) ELF binary, compid extracts build fingerprints
//! (instruction mix, register mix, immediate constants, byte n-grams, section
//! geometry, comment residue) and predicts compiler family, optimization
//! level, architecture, OS, libc, PIE, LTO, and linkage with confidence
//! scores. Predictions come from a small linear model (`model/model.json`)
//! that the Python trainer in `training/` produces from a self-generated
//! build-fingerprint corpus.

pub mod elfinfo;
pub mod features;
pub mod manifest;
pub mod model;
pub mod report;
