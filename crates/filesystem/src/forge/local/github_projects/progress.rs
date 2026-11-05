//! Incremental parsing of the progress stream Git writes to standard error.

use domain::workspace::provisioning::{GithubClonePhase, GithubCloneProgress};

/// Bytes retained from an unterminated progress line before it is discarded.
const MAX_PARTIAL_BYTES: usize = 4 * 1024;

/// Collects distinct, monotonic progress observations from Git's stderr stream.
#[derive(Debug, Default)]
pub(super) struct GitCloneProgressParser {
    partial: String,
    last: Option<GithubCloneProgress>,
}

impl GitCloneProgressParser {
    /// Consume newly written stderr bytes and report each changed observation.
    pub(super) fn push(&mut self, bytes: &[u8], report: &mut dyn FnMut(GithubCloneProgress)) {
        if bytes.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.partial);
        pending.push_str(&String::from_utf8_lossy(bytes));
        let mut start = 0;
        for (index, character) in pending.char_indices() {
            if character == '\r' || character == '\n' {
                self.report(&pending[start..index], report);
                start = index + character.len_utf8();
            }
        }
        let tail = &pending[start..];
        let keep = tail.len().min(MAX_PARTIAL_BYTES);
        let mut boundary = tail.len() - keep;
        while !tail.is_char_boundary(boundary) {
            boundary += 1;
        }
        tail[boundary..].clone_into(&mut self.partial);
    }

    fn report(&mut self, line: &str, report: &mut dyn FnMut(GithubCloneProgress)) {
        let Some(update) = parse_line(line) else {
            return;
        };
        if let Some(last) = self.last
            && (update == last || update.overall_percent() < last.overall_percent())
        {
            return;
        }
        self.last = Some(update);
        report(update);
    }
}

fn parse_line(line: &str) -> Option<GithubCloneProgress> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let line = line.strip_prefix("remote: ").unwrap_or(line);
    let (phase, marker) = phased_marker(line)?;
    let details = line.get(marker.len()..)?.trim_start();
    let percent = match details.split_once('%') {
        Some((value, _)) => Some(
            value
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|percent| *percent <= 100)?,
        ),
        None => None,
    };
    Some(GithubCloneProgress::new(phase, percent))
}

fn phased_marker(line: &str) -> Option<(GithubClonePhase, &'static str)> {
    let markers = [
        ("Receiving objects:", GithubClonePhase::Receiving),
        ("Resolving deltas:", GithubClonePhase::Resolving),
        ("Updating files:", GithubClonePhase::Checkout),
        ("Checking out files:", GithubClonePhase::Checkout),
        ("Compressing objects:", GithubClonePhase::Compressing),
        ("Counting objects:", GithubClonePhase::Counting),
        ("Enumerating objects:", GithubClonePhase::Counting),
    ];
    markers
        .into_iter()
        .find(|(marker, _)| line.starts_with(marker))
        .map(|(marker, phase)| (phase, marker))
}

#[cfg(test)]
mod tests;
