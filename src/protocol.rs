//! Provider-neutral wire format. Schemas are bundled; validation performs no network I/O.
use crate::model::Result;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub const PROPOSAL_SCHEMA: &str = include_str!("../schemas/proposal-v1.schema.json");
pub const IMPLEMENTED_SCHEMA: &str = include_str!("../schemas/proposal-v2.schema.json");
pub const REVIEW_SCHEMA: &str = include_str!("../schemas/review-v1.schema.json");
pub const REVIEW_HISTORY_SCHEMA: &str = include_str!("../schemas/review-v2.schema.json");
static PROPOSAL_VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
static IMPLEMENTED_VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
static REVIEW_VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
static REVIEW_HISTORY_VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

fn validate(value: &Value, schema: &str, cache: &OnceLock<jsonschema::Validator>) -> Result<()> {
    let validator = cache.get_or_init(|| {
        jsonschema::validator_for(
            &serde_json::from_str::<Value>(schema).expect("Bundled schema is JSON"),
        )
        .expect("Bundled schema is valid")
    });
    let errors: Vec<_> = validator
        .iter_errors(value)
        .take(8)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    if !errors.is_empty() {
        return Err(format!("Schema validation failed:\n{}", errors.join("\n")).into());
    }
    Ok(())
}

pub fn validate_review(value: &Value) -> Result<()> {
    match value.get("version").and_then(Value::as_u64) {
        Some(1) => validate(value, REVIEW_SCHEMA, &REVIEW_VALIDATOR),
        Some(2) => validate(value, REVIEW_HISTORY_SCHEMA, &REVIEW_HISTORY_VALIDATOR),
        _ => Err("Expected review version 1 or 2".into()),
    }
}

pub fn parse_proposal(value: &Value) -> Result<Document> {
    match value.get("version").and_then(Value::as_u64) {
        Some(1) => validate(value, PROPOSAL_SCHEMA, &PROPOSAL_VALIDATOR)?,
        Some(2) => validate(value, IMPLEMENTED_SCHEMA, &IMPLEMENTED_VALIDATOR)?,
        _ => return Err("Expected proposal version 1 or 2".into()),
    }
    Ok(serde_json::from_value(value.clone())?)
}

/// Whitespace and object key order do not invalidate a review; array order does.
/// This is the protocol's sorted-key compact JSON representation, not RFC 8785.
/// The viewer computes the digest; generators never need to produce one.
pub fn digest(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_unstable_by_key(|(key, _)| *key);
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(k, v)| (k.clone(), sorted(v)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&sorted(value)).expect("JSON value serializes"))
    )
}

#[derive(Deserialize)]
pub struct Document {
    pub version: u8,
    pub implementation: Option<Implementation>,
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub request: String,
    pub overview: Overview,
    pub base_commit: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Deserialize)]
pub struct Overview {
    pub summary: String,
    pub architecture: String,
    pub questions: Vec<String>,
}

#[derive(Deserialize)]
pub struct Target {
    pub path: String,
    pub line: Option<usize>,
    pub end_line: Option<usize>,
    pub context: Option<String>,
}

#[derive(Deserialize)]
pub struct Annotation {
    pub id: String,
    pub title: String,
    pub operation: String,
    pub target: Target,
    pub intent: String,
    pub rationale: String,
    pub alternatives: Vec<String>,
    pub related: Vec<Related>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Related {
    Annotation {
        label: String,
        annotation_id: String,
    },
    Code {
        label: String,
        target: Target,
    },
}

#[derive(Deserialize)]
pub struct Implementation {
    pub files: Option<Vec<ImplementedFile>>,
    pub source: Option<String>,
    pub head_commit: Option<String>,
}
#[derive(Deserialize)]
pub struct ImplementedFile {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
