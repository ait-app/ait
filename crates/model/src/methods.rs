//! Shared method metadata declared by capability components.

/// Direction and correlation behavior of an inbound method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundKind {
    /// A correlated request that receives a response or protocol error.
    Request,
    /// An uncorrelated client event.
    Event,
    /// A client response to work initiated by the server.
    Response,
}

/// One component-owned method and its inbound message behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodSpec {
    /// Ait method name accepted on the wire.
    pub name: &'static str,
    /// Inbound message behavior.
    pub kind: InboundKind,
}

impl MethodSpec {
    /// Declare a correlated client request.
    ///
    /// # Arguments
    /// * `name` - Exact Ait wire method name.
    ///
    /// # Returns
    /// Static request metadata for the owning component.
    #[must_use]
    pub const fn request(name: &'static str) -> Self {
        Self {
            name,
            kind: InboundKind::Request,
        }
    }

    /// Declare an uncorrelated client event.
    ///
    /// # Arguments
    /// * `name` - Exact Ait wire method name.
    ///
    /// # Returns
    /// Static event metadata for the owning component.
    #[must_use]
    pub const fn event(name: &'static str) -> Self {
        Self {
            name,
            kind: InboundKind::Event,
        }
    }

    /// Declare a client response to work initiated by the server.
    ///
    /// # Arguments
    /// * `name` - Exact Ait wire method name.
    ///
    /// # Returns
    /// Static response metadata for the owning component.
    #[must_use]
    pub const fn response(name: &'static str) -> Self {
        Self {
            name,
            kind: InboundKind::Response,
        }
    }
}

#[cfg(test)]
mod tests;
