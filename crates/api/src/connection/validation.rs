//! Method names, envelope directions and negotiated capability checks.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use protocol::ErrorCode;
use protocol::methods::{InboundKind, PASEO_METHODS};

use crate::capabilities::implemented_methods;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Method {
    pub(super) kind: InboundKind,
    pub(super) capability: &'static str,
}

fn methods() -> &'static BTreeMap<&'static str, Method> {
    static METHODS: OnceLock<BTreeMap<&'static str, Method>> = OnceLock::new();
    METHODS.get_or_init(|| {
        let mut methods = BTreeMap::new();
        for spec in PASEO_METHODS {
            let previous = methods.insert(
                spec.canonical_name,
                Method {
                    kind: spec.kind,
                    capability: spec.canonical_name,
                },
            );
            if let Some(previous) = previous {
                assert_eq!(previous.kind, spec.kind, "conflicting method direction");
            }
        }
        for method in implemented_methods() {
            methods.entry(method).or_insert(Method {
                kind: InboundKind::Request,
                capability: method,
            });
        }
        let previous = methods.insert(
            "server.status.unsubscribe",
            Method {
                kind: InboundKind::Request,
                capability: "server.status.subscribe",
            },
        );
        assert!(previous.is_none(), "duplicate status unsubscribe method");
        methods
    })
}

pub(super) fn lookup(method: &str) -> Option<Method> {
    methods().get(method).copied()
}

pub(super) fn request(
    method: &str,
    implemented: &[String],
    negotiated: &[String],
) -> Result<(), ErrorCode> {
    let metadata = lookup(method).ok_or(ErrorCode::MethodNotFound)?;
    if metadata.kind != InboundKind::Request {
        return Err(ErrorCode::InvalidMessage);
    }
    if !negotiated
        .iter()
        .any(|capability| capability == metadata.capability)
    {
        return Err(ErrorCode::UnsupportedCapability);
    }
    if method != "server.status.unsubscribe"
        && !implemented.iter().any(|implemented| implemented == method)
    {
        return Err(ErrorCode::NotImplemented);
    }
    Ok(())
}

pub(super) fn placeholder(method: &str, kind: InboundKind, negotiated: &[String]) -> ErrorCode {
    let Some(metadata) = lookup(method) else {
        return ErrorCode::MethodNotFound;
    };
    if metadata.kind != kind {
        return ErrorCode::InvalidMessage;
    }
    if !negotiated
        .iter()
        .any(|capability| capability == metadata.capability)
    {
        return ErrorCode::UnsupportedCapability;
    }
    ErrorCode::NotImplemented
}

#[cfg(test)]
mod tests;
