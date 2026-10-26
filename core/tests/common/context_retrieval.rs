//! Test-only sentence retrieval shared by the reading and entry-selection experiments.
//!
//! The model, its digest, and exact inputs identify cached vectors. A failed batch is an error,
//! never an empty vector that looks like abstention. No app code loads this module or its cache.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Ignore whitespace and Unicode presentation differences when excluding a query from evidence.
pub fn sentence_key(text: &str) -> String {
    text.nfkc().filter(|c| !c.is_whitespace()).collect()
}

pub fn scratch() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scratch/context-plan")
}

/// Fail closed on an absent model, without asking Ollama to download anything.
pub fn installed_digest() -> Option<String> {
    let url = format!("{}/api/tags", crate::common::endpoint());
    let mut response = ureq::get(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .call()
        .ok()?;
    let value: serde_json::Value = response.body_mut().read_json().ok()?;
    value["models"]
        .as_array()?
        .iter()
        .find(|model| model["name"].as_str() == Some(crate::common::EMBEDDER))?["digest"]
        .as_str()
        .map(str::to_owned)
}

pub struct Vectors(BTreeMap<String, Vec<f32>>);

impl Vectors {
    pub fn prepare(texts: impl IntoIterator<Item = String>, digest: &str) -> Self {
        let texts: BTreeSet<String> = texts.into_iter().collect();
        let cache = scratch().join("embeddings");
        std::fs::create_dir_all(&cache).expect("create experiment vector cache");
        let path = |text: &str| {
            let key = format!("{}\0{}\0{}", crate::common::EMBEDDER, digest, text);
            cache.join(format!(
                "{}.f32",
                sakatsu_core::hashing::hex(Sha256::digest(key.as_bytes()))
            ))
        };
        let mut vectors = BTreeMap::new();
        let mut missing = Vec::new();
        for text in texts {
            let cached = std::fs::read(path(&text)).ok().and_then(|bytes| {
                if bytes.is_empty() || bytes.len() % 4 != 0 {
                    return None;
                }
                let vector: Vec<f32> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                valid(&vector).then_some(vector)
            });
            if let Some(vector) = cached {
                vectors.insert(text, vector);
            } else {
                missing.push(text);
            }
        }
        println!(
            "  embedder {} digest {digest}; cached {}, missing {}",
            crate::common::EMBEDDER,
            vectors.len(),
            missing.len()
        );
        for (batch, texts) in missing.chunks(32).enumerate() {
            let fresh = crate::common::embed(texts)
                .expect("embedding request failed; no score was produced");
            assert_eq!(fresh.len(), texts.len(), "one vector per input is required");
            for (text, vector) in texts.iter().zip(fresh) {
                assert!(valid(&vector), "non-finite, empty, or zero embedding");
                let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
                let destination = path(text);
                // Ignored probes can run concurrently in one test binary and request the same
                // sentence. Each writer needs its own staging file before publishing the cache.
                static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
                let temporary = destination.with_extension(format!(
                    "{}.{}.part",
                    std::process::id(),
                    NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::write(&temporary, bytes).expect("write vector cache");
                std::fs::rename(temporary, destination).expect("publish vector cache");
                vectors.insert(text.clone(), vector);
            }
            if batch % 10 == 0 {
                println!(
                    "  embedded {} / {} missing sentences",
                    ((batch + 1) * 32).min(missing.len()),
                    missing.len()
                );
            }
        }
        let dimensions: BTreeSet<usize> = vectors.values().map(Vec::len).collect();
        assert!(
            dimensions.len() <= 1,
            "embedding dimensions changed within the run"
        );
        Self(vectors)
    }

    /// Max exemplar similarity per candidate, then a unique nearest candidate. Every copy of
    /// the query is excluded, including copies filed beneath *other* entries. Ties abstain.
    pub fn nearest<K: Ord + Clone>(
        &self,
        query: &str,
        candidates: &BTreeMap<K, Vec<String>>,
    ) -> Option<(K, f64, usize)> {
        let query_key = sentence_key(query);
        let mut scores: Vec<(K, f64)> = candidates
            .iter()
            .filter_map(|(key, examples)| {
                examples
                    .iter()
                    .filter(|example| sentence_key(example) != query_key)
                    .map(|example| crate::common::cosine(&self.0[query], &self.0[example]))
                    .max_by(f64::total_cmp)
                    .map(|score| (key.clone(), score))
            })
            .collect();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        if scores.len() < 2 {
            return None;
        }
        let margin = scores[0].1 - scores[1].1;
        (margin > 1e-6).then(|| (scores[0].0.clone(), margin, scores.len()))
    }
}

fn valid(vector: &[f32]) -> bool {
    !vector.is_empty() && vector.iter().all(|v| v.is_finite()) && vector.iter().any(|v| *v != 0.0)
}

#[derive(Default, Debug)]
pub struct Score {
    pub cases: usize,
    pub baseline_right: usize,
    pub decided: usize,
    pub model_right: usize,
    pub fixed: usize,
    pub broken: usize,
    pub random_expected: f64,
}

impl Score {
    pub fn observe(&mut self, baseline: bool, model: Option<bool>, random: f64) {
        self.cases += 1;
        self.baseline_right += usize::from(baseline);
        if let Some(right) = model {
            self.decided += 1;
            self.model_right += usize::from(right);
            self.fixed += usize::from(right && !baseline);
            self.broken += usize::from(!right && baseline);
            self.random_expected += random;
        }
    }

    pub fn report(&self, label: &str) {
        let net = self.fixed as i64 - self.broken as i64;
        let after = self.baseline_right as i64 + net;
        println!("  {label}: cases {}, baseline {}/{}, decisions {}, model right {}, random expected {:.2}, fixed {}, broken {}, net {net:+}, with abstention fallback {after}/{}; paired p={:.6}",
            self.cases, self.baseline_right, self.cases, self.decided, self.model_right, self.random_expected, self.fixed, self.broken, self.cases, self.paired_p());
    }

    /// Exact two-sided paired sign test: did the changed answers improve more often than hurt?
    pub fn paired_p(&self) -> f64 {
        let n = self.fixed + self.broken;
        if n == 0 {
            return 1.0;
        }
        // Log probabilities avoid underflow when an evaluation has thousands of disagreements.
        let mut log_term = -(n as f64) * std::f64::consts::LN_2;
        let mut sum = log_term.exp();
        for k in 1..=self.fixed.min(self.broken) {
            log_term += ((n - k + 1) as f64).ln() - (k as f64).ln();
            sum += log_term.exp();
        }
        (2.0 * sum).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_cannot_retrieve_itself_under_any_candidate() {
        let vectors = Vectors(BTreeMap::from([
            ("質問".into(), vec![1.0, 0.0]),
            ("別の文".into(), vec![0.0, 1.0]),
            ("第三の文".into(), vec![0.5, 0.5]),
        ]));
        let candidates = BTreeMap::from([
            (1, vec!["質 問".into()]),
            (2, vec!["質問".into(), "別の文".into()]),
            (3, vec!["第三の文".into()]),
        ]);
        let (winner, _, represented) = vectors.nearest("質問", &candidates).unwrap();
        assert_eq!(winner, 3);
        assert_eq!(represented, 2);
        assert!(vectors
            .nearest(
                "質問",
                &BTreeMap::from([(1, vec!["質問".into()]), (2, vec!["別の文".into()])])
            )
            .is_none());
    }

    #[test]
    fn indistinguishable_examples_do_not_choose_by_entry_order() {
        let vectors = Vectors(BTreeMap::from([
            ("q".into(), vec![1.0]),
            ("a".into(), vec![1.0]),
        ]));
        assert!(vectors
            .nearest(
                "q",
                &BTreeMap::from([(1, vec!["a".into()]), (2, vec!["a".into()])])
            )
            .is_none());
        assert_eq!(Score::default().paired_p(), 1.0);
        assert!(
            (Score {
                fixed: 5,
                ..Score::default()
            }
            .paired_p()
                - 0.0625)
                .abs()
                < 1e-12
        );
    }
}
