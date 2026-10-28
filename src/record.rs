use crate::identity::Identity;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub(crate) struct Metric {
    name: String,
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<&'static str>,
}

impl Metric {
    pub(crate) fn new(name: &str, value: Value) -> Self {
        Self {
            name: name.into(),
            value,
            unit: None,
        }
    }

    pub(crate) fn unit(name: &str, value: Value, unit: &'static str) -> Self {
        Self {
            name: name.into(),
            value,
            unit: Some(unit),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Record {
    pub(crate) namespace: &'static str,
    pub(crate) dimension: BTreeMap<String, String>,
    pub(crate) timestamp: u64,
    pub(crate) batch: Vec<Metric>,
}

impl Record {
    pub(crate) fn metrics(identity: &Identity, batch: Vec<Metric>) -> Self {
        Self {
            namespace: "qce/cvm",
            dimension: identity.dimensions(),
            timestamp: timestamp(),
            batch,
        }
    }

    pub(crate) fn heartbeat(identity: &Identity) -> Self {
        Self {
            namespace: "qce/heartbeat",
            dimension: identity.dimensions(),
            timestamp: timestamp(),
            batch: vec![Metric::new("barad_agent_hb", 1.into())],
        }
    }
}

pub(crate) fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
