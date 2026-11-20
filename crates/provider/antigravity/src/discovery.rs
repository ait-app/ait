use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::{AntigravityClient, PROVIDER, transport};
use provider_core::ports::agent_session::AgentSessionError;

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
        .find(|program| provider_native::configuration::executable(program))
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

/// Effort names accepted by `agy --effort`, in ascending order.
pub(super) const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Native `--model` and `--effort` values for one session; `None` leaves AGY's own default.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
}

/// Models offered by the installed CLI, with each model's native effort variants grouped.
#[derive(Debug)]
pub(super) struct Catalog {
    models: Vec<Model>,
}

#[derive(Debug)]
struct Model {
    id: String,
    label: String,
    /// Offered efforts in native listing order.
    efforts: Vec<Effort>,
}

#[derive(Debug)]
struct Effort {
    /// Index into [`EFFORTS`].
    rank: usize,
    label: String,
}

impl Catalog {
    /// Number of distinct models, counting a model's effort variants once.
    pub(super) fn len(&self) -> usize {
        self.models.len()
    }

    /// Provider snapshot entries: one per model, its efforts as thinking options.
    pub(super) fn models(&self) -> Vec<Value> {
        self.models.iter().map(Model::projection).collect()
    }

    /// Resolve the configured model and thinking option to native arguments.
    ///
    /// A model may be a grouped model ID, or a full native variant slug such as
    /// `gemini-test-low` saved before variants were grouped; an explicit thinking option
    /// overrides the effort named by such a slug.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Rejected`] when the model is not offered or does not offer
    /// the requested effort.
    pub(super) fn select(
        &self,
        config: &StoredAgentConfig,
    ) -> Result<Selection, AgentSessionError> {
        let requested = config.thinking_option_id.as_deref();
        let Some(id) = config.model.as_deref() else {
            return Ok(Selection {
                model: None,
                effort: requested.map(str::to_owned),
            });
        };
        let (model, named) = self.find(id).ok_or(AgentSessionError::Rejected)?;
        let effort = requested.or(named);
        if effort.is_some_and(|effort| model.efforts.iter().all(|own| EFFORTS[own.rank] != effort))
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(Selection {
            model: Some(model.id.clone()),
            effort: effort.map(str::to_owned),
        })
    }

    /// The model with this ID, or the model and effort named by a full native variant slug.
    fn find(&self, id: &str) -> Option<(&Model, Option<&'static str>)> {
        let by_id = |id: &str| self.models.iter().find(|model| model.id == id);
        by_id(id).map(|model| (model, None)).or_else(|| {
            EFFORTS.iter().find_map(|effort| {
                let model = id.strip_suffix(effort)?.strip_suffix('-')?;
                Some((by_id(model)?, Some(*effort)))
            })
        })
    }
}

impl Model {
    fn projection(&self) -> Value {
        let mut efforts: Vec<_> = self.efforts.iter().collect();
        efforts.sort_unstable_by_key(|effort| effort.rank);
        let options: Vec<_> = efforts
            .iter()
            .map(|effort| json!({"id":EFFORTS[effort.rank],"label":effort.label}))
            .collect();
        let mut model = json!({"provider":PROVIDER,"id":self.id,"label":self.label,
            "isSelectable":true,"thinkingOptions":options});
        // AGY does not report a default effort; its first listed variant keeps the selector
        // and the launched session in agreement.
        if let Some(effort) = self.efforts.first() {
            model["defaultThinkingOptionId"] = json!(EFFORTS[effort.rank]);
        }
        model
    }
}

pub(super) async fn catalog(
    client: &AntigravityClient,
    cwd: &str,
) -> Result<Catalog, AgentSessionError> {
    let bytes = transport::query(client, Path::new(cwd), &[OsStr::new("models")]).await?;
    parse_catalog(std::str::from_utf8(&bytes).map_err(|_| AgentSessionError::Failed)?)
}

fn parse_catalog(output: &str) -> Result<Catalog, AgentSessionError> {
    let mut ids = HashSet::new();
    let mut positions = HashMap::new();
    let mut models: Vec<Model> = Vec::new();
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
            || !ids.insert(id)
            || ids.len() > 4096
        {
            return Err(AgentSessionError::Failed);
        }
        // AGY lists every effort of a model as its own record; group them under the model.
        let (id, label, effort) = match variant(id, label) {
            Some((id, label, effort)) => (id, label, Some(effort)),
            None => (id, label, None),
        };
        let position = *positions.entry(id).or_insert_with(|| {
            models.push(Model {
                id: id.to_owned(),
                label: label.to_owned(),
                efforts: Vec::new(),
            });
            models.len() - 1
        });
        models[position].efforts.extend(effort);
    }
    if models.is_empty() {
        return Err(AgentSessionError::Failed);
    }
    Ok(Catalog { models })
}

/// Split a native effort variant such as `gemini-test-low` / `Gemini Test (Low)` into the
/// model's ID, the model's label and the effort; `None` for a record that names no effort.
fn variant<'a>(id: &'a str, label: &'a str) -> Option<(&'a str, &'a str, Effort)> {
    let (model_label, effort_label) = label.strip_suffix(')')?.rsplit_once(" (")?;
    let rank = EFFORTS
        .iter()
        .position(|effort| effort.eq_ignore_ascii_case(effort_label))?;
    let model = id.strip_suffix(EFFORTS[rank])?.strip_suffix('-')?;
    let model_label = model_label.trim_end();
    (!model.is_empty() && !model_label.is_empty()).then(|| {
        let effort = Effort {
            rank,
            label: effort_label.to_owned(),
        };
        (model, model_label, effort)
    })
}

#[cfg(test)]
mod tests;
