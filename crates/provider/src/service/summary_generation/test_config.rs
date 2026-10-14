//! Mutable preferences supplied to summary tests without metadata storage.

use std::sync::Mutex;

use domain::summary::SummaryError;
use serde_json::{Value, json};

use model::summary::SummaryConfiguration;

#[derive(Debug)]
pub(crate) struct Configuration(Mutex<Value>);

impl Default for Configuration {
    fn default() -> Self {
        Self(Mutex::new(
            json!({"providers":{},"metadataGeneration":{"providers":[]}}),
        ))
    }
}

impl Configuration {
    pub(crate) fn patch(&self, patch: &Value) -> Result<Value, SummaryError> {
        fn merge(current: &mut Value, patch: &Value) {
            if let (Some(current), Some(patch)) = (current.as_object_mut(), patch.as_object()) {
                for (key, value) in patch {
                    if value.is_object() {
                        merge(current.entry(key).or_insert_with(|| json!({})), value);
                    } else {
                        current.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        let mut current = self.0.lock().map_err(|_| SummaryError::Unavailable)?;
        merge(&mut current, patch);
        Ok(current.clone())
    }
}

impl SummaryConfiguration for Configuration {
    fn current(&self) -> Result<Value, SummaryError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn project(&self, _: &str) -> Value {
        Value::Null
    }
}
