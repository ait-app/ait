//! Complete provider service; runtime metadata remains owned by native execution.

use std::sync::Arc;

use metadata::ports::generation::MetadataGenerator;

use crate::service::{agent_execution::AgentExecution, agents::Agents};

/// Required functional parts of the provider service.
#[derive(Debug)]
pub struct Dependencies {
    /// Versioned Agent presets and default selection.
    pub agents: Agents,
    /// Native execution, runtime directory, provider catalog, and timeline.
    pub execution: AgentExecution,
    /// Shared model-backed wording generation used by other components.
    pub metadata_generator: Arc<dyn MetadataGenerator>,
}

/// One complete provider service with independently owned preset storage and execution lanes.
#[derive(Debug)]
pub struct Service {
    dependencies: Dependencies,
}

impl Service {
    /// Construct the service from all required `dependencies` after its worker has been started.
    /// # Returns
    /// A complete installation whose runtime methods are handled by native execution.
    #[must_use]
    pub fn new(dependencies: Dependencies) -> Self {
        Self { dependencies }
    }

    /// Transfer the complete service into the host's dispatch and lifecycle composition.
    /// # Returns
    /// All functional parts, retaining the execution worker and its runtime directory.
    #[must_use]
    pub fn into_dependencies(self) -> Dependencies {
        self.dependencies
    }
}
