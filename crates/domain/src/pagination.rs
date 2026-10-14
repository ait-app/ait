//! Directory sort clauses, precomputed row values and continuation page results.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Scalar values accepted by Paseo's directory cursor codec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SortValue {
    /// Normalized, case-insensitive text.
    Text(String),
    /// Numeric priority or epoch milliseconds.
    Number(i64),
    /// Missing values sort before present values in ascending order.
    Null,
}

/// Direction of one sort clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// A capability-owned field name and its comparison direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    /// Wire field name.
    pub key: String,
    /// Ordering of this field.
    pub direction: Direction,
}

/// A row with precomputed sort values; sorting never clones the payload.
#[derive(Debug)]
pub struct Entry<T> {
    /// Stable identity used to break ties independently of sort direction.
    pub id: String,
    /// Only fields named in the sort clauses are encoded into cursors.
    pub values: BTreeMap<String, SortValue>,
    /// Capability-owned row.
    pub value: T,
}

/// A bounded page and its opaque continuation tokens.
#[derive(Debug)]
pub struct Page<T> {
    /// Rows after the supplied cursor.
    pub entries: Vec<T>,
    /// Continuation after the last returned row, when more rows exist.
    pub next_cursor: Option<String>,
    /// The input cursor, matching Paseo's previous-page metadata.
    pub prev_cursor: Option<String>,
    /// Whether another matching row follows this page.
    pub has_more: bool,
}
