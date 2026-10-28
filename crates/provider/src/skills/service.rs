//! Serialized services coordinating ports.
//!
//! Serialized orchestration skill selection and deletion consent.

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("agent.skills.get_status.request"),
    MethodSpec::request("agent.skills.reconcile.request"),
    MethodSpec::request("agent.skills.uninstall.request"),
    MethodSpec::request("agent.skills.save_selection.request"),
    MethodSpec::request("agent.skills.import_legacy_selection.request"),
];

use model::ErrorCode;
use model::methods::MethodSpec;
use model::rpc::{decode, encode};
use serde_json::{Value, json};

use crate::skills::ports::{ApplyMode, SkillStore};
use crate::skills::protocol::{self, ImportRequest, Kind, Operation, SaveRequest, Selection};

/// Provider-owned skill coordinator; the host serializes access under its shared service lock.
#[derive(Debug)]
pub struct Skills {
    store: Box<dyn SkillStore>,
}

impl Skills {
    /// Install a backend. Recovery runs on each admitted operation before any status is exposed.
    #[must_use]
    pub fn new(store: Box<dyn SkillStore>) -> Self {
        Self { store }
    }

    /// Recover interrupted installation and distribute selected skills before Agent startup.
    /// Explicit uninstall suspends distribution until a manual reconcile or selection save.
    /// # Errors
    /// Returns storage/conflict errors while retaining recovery records and foreign files.
    pub fn distribute(&mut self) -> Result<(), ErrorCode> {
        self.store.recover()?;
        if self.store.automatic()? {
            let selection = self.store.selection()?.unwrap_or_default();
            self.reconcile(&selection, ApplyMode::Automatic)?;
        }
        Ok(())
    }

    /// Handle the canonical skill operations using fresh scans and explicit deletion consent.
    /// # Errors
    /// Returns validation or safe filesystem errors; partial mutations remain recoverable.
    pub(crate) fn execute(&mut self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        self.store.recover()?;
        match method {
            protocol::SAVE_SELECTION => self.save(decode(params)?),
            protocol::IMPORT_LEGACY_SELECTION => {
                let mut request: ImportRequest = decode(params)?;
                request.selection.normalize();
                if let Some(selection) = self.store.selection()? {
                    return Ok(json!({"imported":false,"selection":selection}));
                }
                self.store.import(&request.selection)?;
                Ok(json!({"imported":true,"selection":request.selection}))
            }
            protocol::GET_STATUS | protocol::RECONCILE | protocol::UNINSTALL => {
                if !params.as_object().is_some_and(serde_json::Map::is_empty) {
                    return Err(ErrorCode::InvalidMessage);
                }
                let selection = self.store.selection()?.unwrap_or_default();
                match method {
                    protocol::UNINSTALL => {
                        let ops: Vec<Operation> = self
                            .store
                            .scan(&selection)?
                            .installed
                            .into_iter()
                            .map(|name| Operation {
                                kind: Kind::Delete,
                                name,
                            })
                            .collect();
                        self.store.apply(&selection, &ops, ApplyMode::Uninstall)?;
                    }
                    protocol::RECONCILE => self.reconcile(&selection, ApplyMode::Reconcile)?,
                    _ => {}
                }
                encode(self.store.scan(&selection)?, ErrorCode::RegistryIo)
            }
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    fn reconcile(&mut self, selection: &Selection, mode: ApplyMode) -> Result<(), ErrorCode> {
        let ops: Vec<_> = self
            .store
            .scan(selection)?
            .ops
            .into_iter()
            .filter(|op| op.kind != Kind::Delete)
            .collect();
        if mode == ApplyMode::Automatic && ops.is_empty() {
            return Ok(());
        }
        self.store.apply(selection, &ops, mode)
    }

    fn save(&mut self, mut request: SaveRequest) -> Result<Value, ErrorCode> {
        request.selection.normalize();
        let previous = self.store.selection()?.unwrap_or_default();
        let plan = self.store.scan(&request.selection)?;
        let removals: Vec<_> = plan
            .ops
            .iter()
            .filter(|op| op.kind == Kind::Delete)
            .map(|op| op.name.clone())
            .collect();
        let confirmed: std::collections::BTreeSet<_> = request
            .confirmed_removals
            .iter()
            .map(|name| name.trim())
            .collect();
        if removals
            .iter()
            .any(|name| !confirmed.contains(name.as_str()))
        {
            let mut value = encode(self.store.scan(&previous)?, ErrorCode::RegistryIo)?;
            value["confirmationRequired"] = json!({"removals":removals});
            return Ok(value);
        }
        self.store
            .apply(&request.selection, &plan.ops, ApplyMode::Save)?;
        let mut value = encode(self.store.scan(&request.selection)?, ErrorCode::RegistryIo)?;
        value["confirmationRequired"] = Value::Null;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
