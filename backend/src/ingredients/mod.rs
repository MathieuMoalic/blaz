//! Canonical ingredient pipeline.
//!
//! This module owns ingredient identity: deterministic quantity/unit
//! parsing, semantic Food resolution, and the food/alias catalog. Recipe
//! wording and preparation stay on recipe ingredients; only the stable
//! identity and canonical metadata live here.

pub mod catalog;
