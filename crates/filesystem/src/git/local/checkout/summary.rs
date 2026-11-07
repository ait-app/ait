//! Lightweight sidebar statistics without allocating structured diff hunks.
//!
//! One refresh runs about seven Git commands: `rev-parse` for the checkout layout, a porcelain v2
//! `status` for HEAD, branch, upstream counts, dirtiness and untracked paths, `remote -v`,
//! `for-each-ref` for default-branch facts, then the base comparison and `diff --numstat`.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use domain::workspace::runtime::{WorkspaceDiffStat, WorkspaceGitSnapshot};

use super::{
    CheckoutFailureKind, CheckoutRuntime, CheckoutRuntimeError, DIFF_OUTPUT_LIMIT, LocalCheckout,
    OUTPUT_EXCEEDED, SMALL_OUTPUT_LIMIT, canonical_git_path, checkout_error, compare_refs,
    comparison_base_with, default_branch_from, diff_head, expanded_directory, git_optional,
    git_required, main_repo_root, path_text, ref_exists, run_git, validate_relative_path,
};
use crate::git::ports::checkout::{AheadBehind, CheckoutStatus};

const ORIGIN_HEAD: &str = "refs/remotes/origin/HEAD";

#[derive(Debug, Default)]
/// Checkout presentation facts and commit identity used to invalidate Forge observations.
pub(crate) struct GitSummary {
    /// Latest Git facts, absent for a non-Git directory.
    pub(crate) git: Option<WorkspaceGitSnapshot>,
    /// Current commit, absent for an unborn checkout.
    pub(crate) head: Option<String>,
    /// Git directories whose changes can alter these facts, absent for a non-Git directory.
    pub(crate) git_dirs: Option<GitDirs>,
}

/// Canonical top level and Git directories of one checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitDirs {
    /// Top level of the work tree, which status covers even when `cwd` is a subdirectory.
    pub(crate) work_tree: PathBuf,
    /// Per-worktree directory holding `HEAD` and the index.
    pub(crate) git_dir: PathBuf,
    /// Directory shared by all worktrees, holding refs and configuration.
    pub(crate) common_dir: PathBuf,
}

impl LocalCheckout {
    /// Read Git status and bounded sidebar statistics for `cwd` without constructing diff hunks.
    /// Non-Git directories return an empty summary; unavailable statistics preserve Git status.
    ///
    /// # Errors
    /// Returns invalid-directory, Git-command, or bounded-output failures from status/identity reads.
    pub(crate) fn sidebar_summary(&self, cwd: &str) -> Result<GitSummary, CheckoutRuntimeError> {
        let cwd = expanded_directory(cwd)?;
        let Some((git_dirs, prefix)) = layout(&cwd)? else {
            return Ok(GitSummary::default());
        };
        let main_repo_root = main_repo_root(&git_dirs.common_dir);
        let is_managed_worktree =
            self.is_managed_worktree(&git_dirs.work_tree, main_repo_root.as_ref());
        let Some(status) = WorkingStatus::read(&cwd)? else {
            return self.oversized_summary(&cwd, git_dirs);
        };
        let remote_url = preferred_remote_url(&cwd)?;
        let refs = RefFacts::read(&cwd, status.branch.as_deref())?;
        let origin_head = refs.origin_head(&cwd, &git_dirs.common_dir)?;
        let base_ref = default_branch_from(
            origin_head.as_deref(),
            status.branch.as_deref(),
            |reference| refs.contains(reference),
        );
        let (ahead_behind, comparison) = match &status.head {
            Some(head) => {
                let compared = compare_base(&cwd, &refs, base_ref.as_deref(), &status)?;
                (
                    compared.ahead_behind,
                    compared.merge_base.unwrap_or_else(|| head.clone()),
                )
            }
            // Unborn checkouts have nothing to compare; their changes diff against the empty tree.
            None => (None, diff_head(&cwd)?),
        };
        // Status names every untracked path from the top level; only those below `cwd` count,
        // as `ls-files --others` run in `cwd` reported them.
        let untracked = status
            .untracked
            .iter()
            .filter(|path| path.starts_with(prefix.as_str()));
        let diff_stat = statistics(&cwd, &git_dirs.work_tree, &comparison, untracked).ok();
        Ok(GitSummary {
            git: Some(WorkspaceGitSnapshot {
                current_branch: status.branch,
                remote_url,
                is_managed_worktree,
                is_dirty: Some(status.dirty),
                ahead_behind: ahead_behind.map(|counts| (counts.ahead, counts.behind)),
                ahead_of_origin: status.upstream.map(|counts| counts.ahead),
                behind_of_origin: status.upstream.map(|counts| counts.behind),
                diff_stat: diff_stat.filter(|stat| stat.additions != 0 || stat.deletions != 0),
            }),
            head: status.head,
            git_dirs: Some(git_dirs),
        })
    }

    /// Facts for a checkout whose status listing exceeds its budget: it is certainly dirty, and
    /// the full status read supplies the remaining fields without line statistics.
    fn oversized_summary(
        &self,
        cwd: &Path,
        git_dirs: GitDirs,
    ) -> Result<GitSummary, CheckoutRuntimeError> {
        let status: CheckoutStatus = self.status(&path_text(cwd)?)?;
        let head = git_optional(cwd, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        Ok(GitSummary {
            git: Some(WorkspaceGitSnapshot {
                current_branch: status.current_branch,
                remote_url: status.remote_url,
                is_managed_worktree: status.is_managed_worktree,
                is_dirty: status.is_dirty,
                ahead_behind: status
                    .ahead_behind
                    .map(|counts| (counts.ahead, counts.behind)),
                ahead_of_origin: status.ahead_of_origin,
                behind_of_origin: status.behind_of_origin,
                diff_stat: None,
            }),
            head,
            git_dirs: Some(git_dirs),
        })
    }
}

/// Canonical top level and Git directories plus the `cwd` prefix within the work tree (empty or
/// ending in `/`), or `None` outside a work tree.
fn layout(cwd: &Path) -> Result<Option<(GitDirs, String)>, CheckoutRuntimeError> {
    let Some(output) = git_optional(
        cwd,
        &[
            "rev-parse",
            "--show-toplevel",
            "--path-format=absolute",
            "--git-common-dir",
            "--absolute-git-dir",
            "--show-prefix",
        ],
    )?
    else {
        return Ok(None);
    };
    let mut lines = output.lines();
    let mut paths = lines.by_ref().take(3).map(canonical_git_path);
    let (Some(repo_root), Some(common_dir), Some(git_dir)) =
        (paths.next(), paths.next(), paths.next())
    else {
        return Err(checkout_error(
            CheckoutFailureKind::Unknown,
            "Unexpected Git layout output",
        ));
    };
    // The prefix is the last line, which output trimming removes at the top level.
    let prefix = lines.next().unwrap_or_default().to_owned();
    Ok(Some((
        GitDirs {
            work_tree: repo_root?,
            git_dir: git_dir?,
            common_dir: common_dir?,
        },
        prefix,
    )))
}

/// Facts parsed from `git status --porcelain=v2 --branch -z --untracked-files=all`.
#[derive(Debug, Default, PartialEq, Eq)]
struct WorkingStatus {
    head: Option<String>,
    branch: Option<String>,
    /// Counts against the configured upstream, absent without a valid upstream.
    upstream: Option<AheadBehind>,
    dirty: bool,
    untracked: Vec<String>,
}

impl WorkingStatus {
    /// Read the porcelain status, or `None` when it exceeds the output budget.
    fn read(cwd: &Path) -> Result<Option<Self>, CheckoutRuntimeError> {
        match run_git(
            cwd,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
            ],
            &[0],
            DIFF_OUTPUT_LIMIT,
        ) {
            Ok(output) => Ok(Some(Self::parse(&output.stdout))),
            Err(error) if error.message == OUTPUT_EXCEEDED => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn parse(output: &str) -> Self {
        let mut status = Self::default();
        let mut records = output.split('\0');
        while let Some(record) = records.next() {
            if let Some(header) = record.strip_prefix("# ") {
                status.header(header);
                continue;
            }
            match record.as_bytes().first() {
                Some(b'1' | b'u') => status.dirty = true,
                // Renames and copies carry their original path in the next record.
                Some(b'2') => {
                    status.dirty = true;
                    records.next();
                }
                Some(b'?') => {
                    status.dirty = true;
                    status.untracked.extend(record.get(2..).map(str::to_owned));
                }
                _ => {}
            }
        }
        status
    }

    fn header(&mut self, header: &str) {
        if let Some(oid) = header.strip_prefix("branch.oid ") {
            self.head = (oid != "(initial)").then(|| oid.to_owned());
        } else if let Some(name) = header.strip_prefix("branch.head ") {
            self.branch = (name != "(detached)").then(|| name.to_owned());
        } else if let Some(counts) = header.strip_prefix("branch.ab ") {
            let mut fields = counts.split_whitespace();
            let ahead = fields.next().and_then(|value| value.strip_prefix('+'));
            let behind = fields.next().and_then(|value| value.strip_prefix('-'));
            self.upstream = ahead
                .and_then(|value| value.parse().ok())
                .zip(behind.and_then(|value| value.parse().ok()))
                .map(|(ahead, behind)| AheadBehind { ahead, behind });
        }
    }
}

/// URL of `origin`, else of the first remote, with `insteadOf` rewrites applied.
fn preferred_remote_url(cwd: &Path) -> Result<Option<String>, CheckoutRuntimeError> {
    let listing = git_required(cwd, &["remote", "-v"], SMALL_OUTPUT_LIMIT)?;
    let remotes: Vec<(&str, &str)> = listing
        .lines()
        .filter_map(|line| line.strip_suffix(" (fetch)")?.split_once('\t'))
        .collect();
    Ok(remotes
        .iter()
        .find(|(name, _)| *name == "origin")
        .or_else(|| remotes.first())
        .map(|(_, url)| url.trim())
        .filter(|url| !url.is_empty())
        .map(str::to_owned))
}

/// Default-branch references read with one `for-each-ref`; other names fall back to Git.
#[derive(Debug)]
struct RefFacts {
    queried: Vec<String>,
    /// Existing queried references and their short symbolic targets (empty when direct).
    found: BTreeMap<String, String>,
}

impl RefFacts {
    fn read(cwd: &Path, branch: Option<&str>) -> Result<Self, CheckoutRuntimeError> {
        let mut queried: Vec<String> = [
            ORIGIN_HEAD,
            "refs/heads/main",
            "refs/heads/master",
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
        ]
        .map(str::to_owned)
        .into();
        queried.extend(branch.map(|branch| format!("refs/remotes/origin/{branch}")));
        let mut arguments = vec!["for-each-ref", "--format=%(refname)%09%(symref:short)"];
        arguments.extend(queried.iter().map(String::as_str));
        let listing = git_required(cwd, &arguments, SMALL_OUTPUT_LIMIT)?;
        // Output is trimmed, so the final direct reference may lack its trailing separator.
        let found = listing
            .lines()
            .map(|line| line.split_once('\t').unwrap_or((line, "")))
            .filter(|(name, _)| queried.iter().any(|reference| reference == name))
            .map(|(name, target)| (name.to_owned(), target.to_owned()))
            .collect();
        Ok(Self { queried, found })
    }

    /// Whether a queried reference exists.
    fn contains(&self, reference: &str) -> bool {
        self.found.contains_key(reference)
    }

    /// Whether `reference` exists, asking Git only for names outside the query.
    fn exists(&self, cwd: &Path, reference: &str) -> Result<bool, CheckoutRuntimeError> {
        if self.queried.iter().any(|queried| queried == reference) {
            Ok(self.contains(reference))
        } else {
            ref_exists(cwd, reference)
        }
    }

    /// Short `origin/HEAD` target, as `git symbolic-ref --short` reports it.
    fn origin_head(
        &self,
        cwd: &Path,
        common_dir: &Path,
    ) -> Result<Option<String>, CheckoutRuntimeError> {
        match self.found.get(ORIGIN_HEAD) {
            Some(target) => Ok((!target.is_empty()).then(|| target.clone())),
            // for-each-ref skips a symbolic ref whose target is missing; only Git resolves it.
            None if common_dir.join(ORIGIN_HEAD).is_file() => {
                git_optional(cwd, &["symbolic-ref", "--quiet", "--short", ORIGIN_HEAD])
            }
            None => Ok(None),
        }
    }
}

/// Default-branch counts and the merge base the sidebar statistics start from.
struct Comparison {
    ahead_behind: Option<AheadBehind>,
    merge_base: Option<String>,
}

fn compare_base(
    cwd: &Path,
    refs: &RefFacts,
    base_ref: Option<&str>,
    status: &WorkingStatus,
) -> Result<Comparison, CheckoutRuntimeError> {
    let Some(branch) = status.branch.as_deref() else {
        return Ok(Comparison {
            ahead_behind: None,
            merge_base: None,
        });
    };
    let compared = base_ref
        .map(|base| comparison_base_with(base, |reference| refs.exists(cwd, reference)))
        .transpose()?;
    let ahead_behind = compared
        .as_deref()
        .map(|compared| compare_refs(cwd, compared, "HEAD"))
        .transpose()?
        .flatten();
    let merge_base = match (base_ref, compared.as_deref()) {
        (Some(base), Some(compared)) if base != branch => {
            git_optional(cwd, &["merge-base", compared, "HEAD"])?
        }
        _ => {
            let tracking = format!("refs/remotes/origin/{branch}");
            if refs.exists(cwd, &tracking)? {
                git_optional(cwd, &["merge-base", &tracking, "HEAD"])?
            } else {
                None
            }
        }
    };
    Ok(Comparison {
        ahead_behind,
        merge_base,
    })
}

/// Tracked line counts against `comparison` plus text lines of `untracked` paths, which are
/// relative to `work_tree`.
fn statistics<'a>(
    cwd: &Path,
    work_tree: &Path,
    comparison: &str,
    untracked: impl Iterator<Item = &'a String>,
) -> Result<WorkspaceDiffStat, CheckoutRuntimeError> {
    let tracked = git_required(
        cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--numstat",
            "-z",
            comparison,
            "--",
        ],
        DIFF_OUTPUT_LIMIT,
    )?;
    let mut result = parse_numstat(&tracked);
    let mut remaining = DIFF_OUTPUT_LIMIT;
    for path in untracked {
        validate_relative_path(path)?;
        result.additions = result
            .additions
            .saturating_add(count_lines(&work_tree.join(path), &mut remaining)?);
    }
    Ok(result)
}

fn parse_numstat(output: &str) -> WorkspaceDiffStat {
    let mut result = WorkspaceDiffStat::default();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        let mut fields = record.splitn(3, '\t');
        let added = fields.next().and_then(|value| value.parse::<u64>().ok());
        let removed = fields.next().and_then(|value| value.parse::<u64>().ok());
        if let (Some(added), Some(removed)) = (added, removed) {
            result.additions = result.additions.saturating_add(added);
            result.deletions = result.deletions.saturating_add(removed);
        }
        if fields.next() == Some("") {
            records.next();
            records.next();
        }
    }
    result
}

fn count_lines(path: &Path, remaining: &mut u64) -> Result<u64, CheckoutRuntimeError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| super::io_error(&error))?;
    if metadata.file_type().is_symlink() {
        return Ok(1);
    }
    if !metadata.is_file() {
        return Ok(0);
    }
    let mut file = File::open(path).map_err(|error| super::io_error(&error))?;
    let mut buffer = [0; 8192];
    let mut lines = 0_u64;
    let mut last = None;
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|error| super::io_error(&error))?;
        if length == 0 {
            return Ok(lines + u64::from(last.is_some_and(|byte| byte != b'\n')));
        }
        *remaining = remaining.checked_sub(length as u64).ok_or_else(|| {
            checkout_error(
                CheckoutFailureKind::Unknown,
                "Untracked statistics exceeded the read budget",
            )
        })?;
        let bytes = &buffer[..length];
        if bytes.contains(&0) {
            return Ok(0);
        }
        lines += memchr::memchr_iter(b'\n', bytes).count() as u64;
        last = bytes.last().copied();
    }
}

#[cfg(test)]
mod tests;
