//! Find user-level npm CLI installations when desktop PATH omits their bin directory.

use std::borrow::Cow;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// Resolve fallback npm bin directories using `environment` and the user's npmrc prefix.
/// Returns configured bins before `~/.npm-global/bin`; returns no paths on non-Unix hosts.
pub(super) fn bins(environment: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    if !cfg!(unix) {
        return Vec::new();
    }
    let mut bins = Vec::with_capacity(2);
    if let Some(prefix) = prefix(&environment) {
        bins.push(prefix.join("bin"));
    }
    if let Some(home) = environment("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
    {
        let conventional = home.join(".npm-global/bin");
        if !bins.contains(&conventional) {
            bins.push(conventional);
        }
    }
    bins
}

fn prefix(environment: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(value) = environment("npm_config_prefix")
        .filter(|value| !value.is_empty())
        .or_else(|| environment("NPM_CONFIG_PREFIX").filter(|value| !value.is_empty()))
    {
        return expand(value.to_str()?, environment);
    }
    let config = environment("npm_config_userconfig")
        .filter(|value| !value.is_empty())
        .or_else(|| environment("NPM_CONFIG_USERCONFIG").filter(|value| !value.is_empty()))
        .or_else(|| {
            environment("HOME").map(|home| PathBuf::from(home).join(".npmrc").into_os_string())
        })?;
    let config = expand(config.to_str()?, environment)?;
    let mut contents = String::new();
    File::open(config)
        .ok()?
        .take(64 * 1024 + 1)
        .read_to_string(&mut contents)
        .ok()?;
    if contents.len() > 64 * 1024 {
        return None;
    }
    let value = contents
        .lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "prefix")
        .map(|(_, value)| value)
        .last()?;
    expand(&decode(value)?, environment)
}

fn decode(value: &str) -> Option<Cow<'_, str>> {
    let value = value.trim();
    if value.starts_with('"') {
        return serde_json::from_str::<String>(value).ok().map(Cow::Owned);
    }
    if let Some(value) = value.strip_prefix('\'') {
        return value.strip_suffix('\'').map(Cow::Borrowed);
    }
    if !value.contains(['\\', ';', '#']) {
        return Some(Cow::Borrowed(value));
    }
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        match character {
            '#' | ';' => break,
            '\\' => match characters.next() {
                Some(escaped @ ('\\' | '#' | ';')) => decoded.push(escaped),
                Some(escaped) => {
                    decoded.push('\\');
                    decoded.push(escaped);
                }
                None => decoded.push('\\'),
            },
            character => decoded.push(character),
        }
    }
    decoded.truncate(decoded.trim_end().len());
    Some(Cow::Owned(decoded))
}

fn expand(value: &str, environment: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let (mut expanded, mut remaining) = if let Some(relative) = value.strip_prefix("~/") {
        let mut home = environment("HOME")?;
        if !std::path::Path::new(&home).is_absolute() {
            return None;
        }
        home.push("/");
        (home, relative)
    } else {
        (OsString::new(), value)
    };
    while let Some(start) = remaining.find("${") {
        expanded.push(&remaining[..start]);
        let variable = &remaining[start + 2..];
        let end = variable.find('}')?;
        let name = &variable[..end];
        let value = if let Some(name) = name.strip_suffix('?') {
            environment(name).unwrap_or_default()
        } else {
            environment(name)?
        };
        expanded.push(value);
        remaining = &variable[end + 1..];
    }
    expanded.push(remaining);
    let path = PathBuf::from(expanded);
    path.is_absolute().then_some(path)
}

#[cfg(all(test, unix))]
mod tests;
