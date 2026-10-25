use std::sync::Arc;

use crate::{Lifecycle, Limits, Runtime, ServerInfo, VERSION};

pub(crate) fn runtime() -> Arc<Runtime> {
    Arc::new(Runtime::new(ServerInfo {
        server_id: "server".to_owned(),
        version: Some(crate::BUILD_VERSION.to_owned()),
        instance_id: "instance".to_owned(),
        listen: "127.0.0.1:7316".to_owned(),
        lifecycle: Lifecycle::Ready,
        protocol: VERSION,
        capabilities: Vec::new(),
        implemented_capabilities: Vec::new(),
        features: Vec::new(),
        limits: Limits::default(),
    }))
}
