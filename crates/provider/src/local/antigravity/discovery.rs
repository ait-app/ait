use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{AntigravityClient, PROVIDER, transport};
use crate::ports::agent_session::AgentSessionError;

pub(super) fn installed_program() -> PathBuf {
    let search = Search {
        path: std::env::var_os("PATH"),
        home: std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from),
        local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        homebrew_prefix: std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from),
    };
    resolve(&search)
}

fn resolve(search: &Search) -> PathBuf {
    candidates(search)
        .into_iter()
        .find(|program| crate::local::configuration::executable(program))
        .unwrap_or_else(|| "agy".into())
}

struct Search {
    path: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    homebrew_prefix: Option<PathBuf>,
}

fn candidates(search: &Search) -> Vec<PathBuf> {
    let executable = if cfg!(windows) { "agy.exe" } else { "agy" };
    let mut programs: Vec<_> = search
        .path
        .as_deref()
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(executable))
        .collect();
    if let Some(home) = &search.home {
        programs.push(home.join(".local/bin").join(executable));
    }
    if let Some(local) = &search.local_app_data {
        programs.push(local.join("agy/bin/agy.exe"));
    } else if cfg!(windows)
        && let Some(home) = &search.home
    {
        programs.push(home.join("AppData/Local/agy/bin/agy.exe"));
    }
    if let Some(prefix) = &search.homebrew_prefix {
        programs.push(prefix.join("bin").join(executable));
    }
    if !cfg!(windows) {
        programs.extend(
            [
                "/opt/homebrew/bin/agy",
                "/usr/local/bin/agy",
                "/home/linuxbrew/.linuxbrew/bin/agy",
            ]
            .map(PathBuf::from),
        );
        if let Some(home) = &search.home {
            programs.push(home.join(".linuxbrew/bin/agy"));
        }
    }
    programs
}

pub(super) async fn models(
    client: &AntigravityClient,
    cwd: &str,
) -> Result<Vec<Value>, AgentSessionError> {
    let bytes = transport::query(client, Path::new(cwd), &[OsStr::new("models")]).await?;
    parse_models(std::str::from_utf8(&bytes).map_err(|_| AgentSessionError::Failed)?)
}

fn parse_models(output: &str) -> Result<Vec<Value>, AgentSessionError> {
    let mut ids = BTreeSet::new();
    let mut models = Vec::new();
    for line in output.lines() {
        // AGY also prints a startup notice; only tab-separated model records are data.
        let Some((id, label)) = line.split_once('\t') else {
            continue;
        };
        let (id, label) = (id.trim(), label.trim());
        if id.is_empty()
            || id.len() > 512
            || label.is_empty()
            || label.len() > 1024
            || id.chars().chain(label.chars()).any(char::is_control)
            || !ids.insert(id.to_owned())
            || models.len() >= 4096
        {
            return Err(AgentSessionError::Failed);
        }
        // Model slugs already include native effort variants. No guessed default or effort list.
        models.push(json!({"provider":PROVIDER,"id":id,"label":label,
            "isSelectable":true,"thinkingOptions":[]}));
    }
    if models.is_empty() {
        return Err(AgentSessionError::Failed);
    }
    Ok(models)
}

#[cfg(test)]
mod tests;
