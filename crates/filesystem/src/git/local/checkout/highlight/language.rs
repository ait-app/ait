//! GitHub-compatible language hints, resolved independently for each comparison side.

use std::collections::BTreeMap;
use std::path::Path;

use super::{SYNTAXES, SyntaxReference, run_git, syntax_for_path, validate_relative_path};
use crate::git::ports::checkout::{DiffLineKind, ParsedDiffFile};

/// Read `linguist-language` attributes from `revision`, or the working tree when absent.
/// Paths are batched and NUL-delimited output preserves spaces and control characters.
pub(super) fn overrides(
    cwd: &Path,
    files: &[ParsedDiffFile],
    revision: Option<&str>,
    side: DiffLineKind,
) -> BTreeMap<String, String> {
    let paths = files
        .iter()
        .filter(|file| !file.hunks.is_empty())
        .map(|file| {
            if side == DiffLineKind::Remove {
                file.old_path.as_deref().unwrap_or(&file.path)
            } else {
                &file.path
            }
        })
        .filter(|path| validate_relative_path(path).is_ok())
        .collect::<Vec<_>>();
    let mut languages = BTreeMap::new();
    for paths in paths.chunks(256) {
        let mut arguments = vec!["check-attr", "-z"];
        if let Some(revision) = revision {
            arguments.extend(["--source", revision]);
        }
        arguments.extend(["linguist-language", "--"]);
        arguments.extend_from_slice(paths);
        if let Ok(output) = run_git(cwd, &arguments, &[0], 1024 * 1024) {
            let mut fields = output.stdout.split('\0');
            while let (Some(path), Some(_attribute), Some(language)) =
                (fields.next(), fields.next(), fields.next())
            {
                if !matches!(language, "unspecified" | "unset" | "set") {
                    languages.insert(path.to_owned(), language.to_owned());
                }
            }
        }
    }
    languages
}

/// Resolve explicit attributes, modelines, known filenames, shebangs, and extensions.
/// Unsupported explicit languages remain plain text instead of using an unrelated grammar.
pub(super) fn detect(
    path: &str,
    content: Option<&str>,
    language: Option<&str>,
) -> Option<&'static SyntaxReference> {
    if let Some(language) = language {
        return syntax_for_language(language);
    }
    let content = content.unwrap_or("");
    let modeline = content
        .lines()
        .take(5)
        .chain(content.lines().rev().take(5))
        .find_map(|line| modeline_language(line).and_then(syntax_for_language));
    modeline
        .or_else(|| syntax_for_filename(path))
        .or_else(|| {
            content
                .lines()
                .next()
                .and_then(|line| SYNTAXES.find_syntax_by_first_line(line))
        })
        .or_else(|| syntax_for_path(path))
        .filter(|syntax| syntax.name != "Plain Text")
}

/// Resolve complete filenames such as `Dockerfile`, `Makefile`, and dotfiles.
pub(super) fn syntax_for_filename(path: &str) -> Option<&'static SyntaxReference> {
    let filename = Path::new(path).file_name()?.to_str()?;
    SYNTAXES
        .find_syntax_by_extension(filename)
        .or_else(|| {
            let (base, _) = filename.split_once('.')?;
            matches!(
                base.to_ascii_lowercase().as_str(),
                "dockerfile" | "makefile"
            )
            .then(|| SYNTAXES.find_syntax_by_extension(base))?
        })
        .filter(|syntax| syntax.name != "Plain Text")
}

fn syntax_for_language(language: &str) -> Option<&'static SyntaxReference> {
    let normalized = language.replace('-', " ");
    let alias = match normalized.to_ascii_lowercase().as_str() {
        "shell" | "shell script" => "sh",
        "c#" => "cs",
        "javascript" | "jsx" => "js",
        "typescript" => "ts",
        "text" => return None,
        _ => normalized.as_str(),
    };
    SYNTAXES
        .find_syntax_by_token(language)
        .or_else(|| SYNTAXES.find_syntax_by_token(alias))
        .filter(|syntax| syntax.name != "Plain Text")
}

fn modeline_language(line: &str) -> Option<&str> {
    let value = if let Some((_, modeline)) = line.split_once("vim:") {
        ["syntax=", "filetype=", "ft="]
            .into_iter()
            .find_map(|key| modeline.split_once(key).map(|(_, value)| value))?
    } else {
        let (_, modeline) = line.split_once("-*-")?;
        let (modeline, _) = modeline.split_once("-*-")?;
        modeline
            .split_once("mode:")
            .map_or(modeline, |(_, value)| value)
            .trim()
    };
    value
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '+' | '#')
        })
        .next()
}

#[cfg(test)]
mod tests;
