use std::borrow::Cow;
use std::path::PathBuf;

use model::ErrorCode;

pub(super) fn key(cwd: Option<&str>) -> Result<Option<String>, ErrorCode> {
    cwd.map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(directory)
        .transpose()
}

pub(super) fn directory(cwd: &str) -> Result<String, ErrorCode> {
    let path = if cwd == "~" {
        home()?
    } else if let Some(suffix) = cwd.strip_prefix("~/") {
        home()?.join(suffix)
    } else {
        PathBuf::from(cwd)
    };
    let path = path.canonicalize().map_err(|_| ErrorCode::InvalidMessage)?;
    if !path.is_dir() {
        return Err(ErrorCode::InvalidMessage);
    }
    path.into_os_string()
        .into_string()
        .map_err(|_| ErrorCode::InvalidMessage)
}

pub(super) fn discovery_cwd(key: Option<&str>) -> Result<Cow<'_, str>, ErrorCode> {
    match key {
        Some(cwd) => Ok(Cow::Borrowed(cwd)),
        None => directory("~").map(Cow::Owned),
    }
}

fn home() -> Result<PathBuf, ErrorCode> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or(ErrorCode::AgentIo)
}
