use crate::{fail, validate_destination, validate_name, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Labels {
    pub schema: u32,
    pub examples: BTreeMap<String, String>,
}
impl Default for Labels {
    fn default() -> Self {
        Self {
            schema: 1,
            examples: BTreeMap::new(),
        }
    }
}
impl Labels {
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.examples.len() > 5000 {
            return fail("unsupported labels or more than 5000 examples");
        }
        for (name, destination) in &self.examples {
            validate_name(name)?;
            validate_destination(destination)?;
        }
        Ok(())
    }
    pub fn predict(&self, name: &str) -> Result<Option<Prediction>> {
        self.validate()?;
        let mut counts: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
        let mut documents: BTreeMap<&str, usize> = BTreeMap::new();
        let mut vocabulary = BTreeSet::new();
        for (example, destination) in &self.examples {
            *documents.entry(destination).or_default() += 1;
            for token in tokens(example) {
                vocabulary.insert(token.clone());
                *counts
                    .entry(destination)
                    .or_default()
                    .entry(token)
                    .or_default() += 1;
            }
        }
        if documents.len() < 2 || documents.values().any(|n| *n < 2) {
            return Ok(None);
        }
        let input: Vec<_> = tokens(name)
            .into_iter()
            .filter(|t| vocabulary.contains(t))
            .collect();
        if input.is_empty() {
            return Ok(None);
        }
        let mut scored = Vec::new();
        for (&destination, &n) in &documents {
            let class = counts.get(destination).ok_or("empty class")?;
            let total: usize = class.values().sum();
            let mut log = ((n + 1) as f64 / (self.examples.len() + documents.len()) as f64).ln();
            for token in &input {
                log += ((*class.get(token).unwrap_or(&0) + 1) as f64
                    / (total + vocabulary.len()) as f64)
                    .ln();
            }
            scored.push((log, destination.to_string()));
        }
        if scored.len() < 2 {
            return Ok(None);
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let (best, destination) = &scored[0];
        let normalizer: f64 = scored.iter().map(|(x, _)| (x - best).exp()).sum();
        let evidence: Vec<_> = input
            .into_iter()
            .filter(|t| {
                let own = *counts[destination.as_str()].get(t).unwrap_or(&0);
                own > 0
                    && counts
                        .iter()
                        .filter(|(d, _)| **d != destination.as_str())
                        .all(|(_, c)| own > *c.get(t).unwrap_or(&0))
            })
            .collect();
        if evidence.is_empty() {
            return Ok(None);
        }
        Ok(Some(Prediction {
            destination: destination.clone(),
            score: 1.0 / normalizer,
            evidence,
        }))
    }
}

/// This is an uncalibrated model score, not a measured probability of correctness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub destination: String,
    pub score: f64,
    pub evidence: Vec<String>,
}
fn tokens(name: &str) -> BTreeSet<String> {
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_lowercase();
    let mut out = BTreeSet::new();
    for word in stem.split(|c: char| !c.is_alphabetic()) {
        if (2..=40).contains(&word.chars().count()) {
            out.insert(format!("word:{word}"));
        }
    }
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        out.insert(format!("ext:{}", extension.to_lowercase()));
    }
    out
}
