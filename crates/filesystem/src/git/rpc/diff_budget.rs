//! Preserve small highlighted diffs by degrading the most expensive files first.

use std::cmp::Reverse;

use super::{clear_diff_hunks, clear_diff_tokens};
use crate::git::protocol::{CheckoutDiffResult, ParsedDiffFile};
use crate::support::budget::{OUTPUT_BUDGET, encoded_size};

struct FileCost {
    index: usize,
    bytes: usize,
    token_bytes: usize,
}

/// Fit a snapshot by limiting large file bodies, then optional syntax tokens.
///
/// # Errors
/// Returns a JSON encoding error; the caller retains the whole-snapshot fallback.
pub(super) fn fit_result(result: &mut CheckoutDiffResult) -> serde_json::Result<()> {
    let mut bytes = encoded_size(result)?;
    let mut costs = result
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            Ok(FileCost {
                index,
                bytes: encoded_size(file)?,
                token_bytes: token_bytes(file)?,
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?;
    // If plain text alone is too large, stripping syntax cannot rescue it.
    // Keep the syntax of small files while replacing the largest bodies.
    let mut plain_bytes = bytes - costs.iter().map(|cost| cost.token_bytes).sum::<usize>();
    costs.sort_by_key(|cost| Reverse(cost.bytes - cost.token_bytes));
    for cost in &mut costs {
        if plain_bytes <= OUTPUT_BUDGET {
            break;
        }
        let file = &mut result.files[cost.index];
        if file.hunks.is_empty() {
            continue;
        }
        clear_diff_hunks(file);
        let placeholder_bytes = encoded_size(file)?;
        plain_bytes = plain_bytes - (cost.bytes - cost.token_bytes) + placeholder_bytes;
        bytes = bytes - cost.bytes + placeholder_bytes;
        cost.token_bytes = 0;
    }
    costs.sort_by_key(|cost| Reverse(cost.token_bytes));
    for cost in costs {
        if bytes <= OUTPUT_BUDGET {
            break;
        }
        clear_diff_tokens(&mut result.files[cost.index]);
        bytes -= cost.token_bytes;
    }
    Ok(())
}

fn token_bytes(file: &ParsedDiffFile) -> serde_json::Result<usize> {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter_map(|line| line.tokens.as_ref())
        .try_fold(0, |bytes, tokens| {
            // Missing optional tokens omit the entire field, including its comma.
            Ok(bytes + b",\"tokens\":".len() + encoded_size(tokens)?)
        })
}
