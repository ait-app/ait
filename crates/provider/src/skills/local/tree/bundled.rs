//! Canonical orchestration resources embedded for desktop and standalone daemon distribution.

use std::collections::{BTreeMap, BTreeSet};

use super::Tree;

const FILES: &[(&str, &[u8])] = &[
    (
        "SKILL.md",
        include_bytes!("../../../../../../skills/ait-orchestration/SKILL.md"),
    ),
    (
        "agents/openai.yaml",
        include_bytes!("../../../../../../skills/ait-orchestration/agents/openai.yaml"),
    ),
    (
        "references/operations.md",
        include_bytes!("../../../../../../skills/ait-orchestration/references/operations.md"),
    ),
    (
        "references/mcp-access.md",
        include_bytes!("../../../../../../skills/ait-orchestration/references/mcp-access.md"),
    ),
];

impl Tree {
    pub(in crate::skills::local) fn orchestration() -> Self {
        Self {
            files: FILES
                .iter()
                .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
                .collect::<BTreeMap<_, _>>(),
            directories: BTreeSet::from(["agents".to_owned(), "references".to_owned()]),
            ..Self::default()
        }
    }
}
