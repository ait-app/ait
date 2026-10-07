//! Method names, envelope directions and negotiated capability checks.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use model::methods::InboundKind;
use protocol::ErrorCode;

use crate::capabilities::implemented_methods;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Method {
    pub(super) kind: InboundKind,
    pub(super) capability: &'static str,
}

fn methods() -> &'static BTreeMap<&'static str, Method> {
    static METHOD_METADATA: OnceLock<BTreeMap<&'static str, Method>> = OnceLock::new();
    METHOD_METADATA.get_or_init(|| {
        let mut methods = BTreeMap::new();
        for spec in implemented_methods() {
            let previous = methods.insert(
                spec.name,
                Method {
                    kind: spec.kind,
                    capability: spec.name,
                },
            );
            assert!(previous.is_none(), "duplicate Ait method: {}", spec.name);
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
