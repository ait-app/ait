//! Host-owned adapter boundaries.

// TODO: Flatten redundant area modules introduced by ADR-131, starting with
// `worktrees::ports::worktrees`. Define WorktreeArchiveCleanup here and audit similar
// single-area layers in metadata, preserving capability groups and port boundaries.
pub mod worktrees;
