//! Server highlighting using per-side languages, comparison snapshots, and hunk fallback.

mod language;

use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

use sha2::{Digest, Sha256};
use two_face::re_exports::syntect::easy::ScopeRegionIterator;
use two_face::re_exports::syntect::parsing::{
    ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet,
};

use super::{run_git, validate_relative_path};
use crate::git::ports::checkout::{DiffLineKind, HighlightToken, ParsedDiffFile};

// Paseo skips pathological single lines. Also bound full-file and cache memory
// because subscriptions poll repeatedly and a small patch can belong to a huge file.
const MAX_LINE_CHARS: usize = 10_000;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 32;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_no_newlines);
static ROLES: LazyLock<Vec<(Scope, &str)>> = LazyLock::new(|| {
    [
        ("comment", "comment"),
        ("punctuation.definition.comment", "comment"),
        ("constant.character.escape", "escape"),
        ("string.regexp", "regexp"),
        ("string", "string"),
        ("punctuation.definition.string", "string"),
        ("constant.numeric", "number"),
        ("constant.language", "literal"),
        ("keyword.operator", "operator"),
        ("keyword", "keyword"),
        ("storage", "keyword"),
        ("entity.name.function", "definition"),
        ("support.function", "function"),
        ("variable.function", "function"),
        ("entity.name.type.class", "class"),
        ("entity.name.type", "type"),
        ("support.type", "type"),
        ("support.class", "class"),
        ("entity.name.tag", "tag"),
        ("entity.other.attribute-name", "attribute"),
        ("variable.other.property", "property"),
        ("variable", "variable"),
        ("punctuation", "punctuation"),
        ("meta.preprocessor", "meta"),
        ("markup.heading", "heading"),
        ("markup.underline.link", "link"),
        ("constant.other", "literal"),
    ]
    .into_iter()
    .map(|(scope, role)| {
        (
            Scope::new(scope).expect("Static syntax scope must be valid"),
            role,
        )
    })
    .collect()
});

type TokenLines = Arc<Vec<Vec<HighlightToken>>>;

#[derive(Debug)]
struct CacheEntry {
    syntax: String,
    key: CacheKey,
    lines: TokenLines,
    bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum CacheKey {
    Content([u8; 32]),
    Snapshot([u8; 32]),
}

#[derive(Debug, Default)]
pub(super) struct DiffHighlighter {
    cache: RwLock<VecDeque<CacheEntry>>,
}

struct TokenLookup {
    first_line: u64,
    lines: TokenLines,
}

struct DiffSide<'a> {
    path: &'a str,
    revision: Option<&'a str>,
    kind: DiffLineKind,
    language: Option<&'a str>,
}

impl TokenLookup {
    fn get(&self, line_number: u64) -> Option<&Vec<HighlightToken>> {
        let index = usize::try_from(line_number.checked_sub(self.first_line)?).ok()?;
        self.lines.get(index)
    }
}

impl DiffHighlighter {
    /// Populate syntax roles from old/new snapshots; failures leave plain diff text intact.
    /// `new_ref: None` denotes the working tree, while commit/base diffs use an explicit ref.
    pub(super) fn highlight_files(
        &self,
        files: &mut [ParsedDiffFile],
        cwd: &Path,
        old_ref: &str,
        new_ref: Option<&str>,
    ) {
        let old_languages = language::overrides(cwd, files, Some(old_ref), DiffLineKind::Remove);
        let new_languages = language::overrides(cwd, files, new_ref, DiffLineKind::Add);
        for file in files {
            let old_path = file.old_path.as_deref().unwrap_or(&file.path);
            let old = DiffSide {
                path: old_path,
                revision: Some(old_ref),
                kind: DiffLineKind::Remove,
                language: old_languages.get(old_path).map(String::as_str),
            };
            let new = DiffSide {
                path: &file.path,
                revision: new_ref,
                kind: DiffLineKind::Add,
                language: new_languages.get(&file.path).map(String::as_str),
            };
            let (old_tokens, new_tokens) = self.highlight(file, cwd, &old, &new);
            apply_tokens(file, old_tokens.as_ref(), new_tokens.as_ref());
        }
    }

    fn highlight(
        &self,
        file: &ParsedDiffFile,
        cwd: &Path,
        old: &DiffSide<'_>,
        new: &DiffSide<'_>,
    ) -> (Option<TokenLookup>, Option<TokenLookup>) {
        if file.hunks.is_empty()
            || file.hunks.iter().flat_map(|hunk| &hunk.lines).any(|line| {
                line.content.len() > MAX_LINE_CHARS && line.content.chars().count() > MAX_LINE_CHARS
            })
        {
            return (None, None);
        }
        let old_tokens = (!file.is_new)
            .then(|| self.side_tokens(file, cwd, old))
            .flatten();
        let new_tokens = (!file.is_deleted)
            .then(|| self.side_tokens(file, cwd, new))
            .flatten();
        (old_tokens, new_tokens)
    }

    fn side_tokens(
        &self,
        file: &ParsedDiffFile,
        cwd: &Path,
        side: &DiffSide<'_>,
    ) -> Option<TokenLookup> {
        if let Some(revision) = side.revision
            && let Some(tokens) = self.snapshot(cwd, side.path, revision, side.language)
        {
            return Some(tokens);
        }
        let content = side
            .revision
            .is_none()
            .then(|| read_content(cwd, side.path, None))
            .flatten();
        let first_line = file.hunks.iter().find_map(|hunk| {
            let first = if side.kind == DiffLineKind::Remove {
                hunk.old_start
            } else {
                hunk.new_start
            };
            (first == 1).then(|| {
                hunk.lines
                    .iter()
                    .find(|line| line.kind == side.kind || line.kind == DiffLineKind::Context)
                    .map(|line| line.content.as_str())
            })?
        });
        let syntax = language::detect(side.path, content.as_deref().or(first_line), side.language)?;
        self.lookup(file, syntax, content.as_deref(), side.kind)
    }

    fn snapshot(
        &self,
        cwd: &Path,
        path: &str,
        revision: &str,
        language: Option<&str>,
    ) -> Option<TokenLookup> {
        let mut digest = Sha256::new();
        for part in [cwd.to_str()?, path, revision, language.unwrap_or("")] {
            digest.update(part.as_bytes());
            digest.update([0]);
        }
        let key = CacheKey::Snapshot(digest.finalize().into());
        let cached = self.cache.read().ok().and_then(|cache| {
            cache
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.lines.clone())
        });
        let lines = if let Some(lines) = cached {
            lines
        } else {
            let content = read_content(cwd, path, Some(revision))?;
            let syntax = language::detect(path, Some(&content), language)?;
            self.tokenize_with_key(&content, syntax, key)?
        };
        Some(TokenLookup {
            first_line: 1,
            lines,
        })
    }

    fn lookup(
        &self,
        file: &ParsedDiffFile,
        syntax: &SyntaxReference,
        content: Option<&str>,
        side: DiffLineKind,
    ) -> Option<TokenLookup> {
        if let Some(content) = content
            && let Some(lines) = self.tokenize(content, syntax)
        {
            return Some(TokenLookup {
                first_line: 1,
                lines,
            });
        }
        let (first_line, content) = reconstruct(file, side)?;
        Some(TokenLookup {
            first_line,
            lines: self.tokenize(&content, syntax)?,
        })
    }

    fn tokenize(&self, content: &str, syntax: &SyntaxReference) -> Option<TokenLines> {
        let key = CacheKey::Content(Sha256::digest(content.as_bytes()).into());
        self.tokenize_with_key(content, syntax, key)
    }

    fn cached(&self, syntax: &SyntaxReference, key: &CacheKey) -> Option<TokenLines> {
        self.cache.read().ok().and_then(|cache| {
            cache
                .iter()
                .find(|entry| entry.syntax == syntax.name && entry.key == *key)
                .map(|entry| entry.lines.clone())
        })
    }

    fn tokenize_with_key(
        &self,
        content: &str,
        syntax: &SyntaxReference,
        key: CacheKey,
    ) -> Option<TokenLines> {
        if content.len() > MAX_FILE_BYTES
            || content
                .split('\n')
                .any(|line| line.len() > MAX_LINE_CHARS && line.chars().count() > MAX_LINE_CHARS)
        {
            return None;
        }
        if let Some(cached) = self.cached(syntax, &key) {
            return Some(cached);
        }
        let lines = Arc::new(tokenize_content(content, syntax)?);
        let bytes = lines
            .iter()
            .map(|line| {
                line.capacity() * size_of::<HighlightToken>()
                    + line
                        .iter()
                        .map(|token| {
                            token.text.capacity() + token.style.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
            + lines.capacity() * size_of::<Vec<HighlightToken>>();
        if bytes <= MAX_CACHE_BYTES
            && let Ok(mut cache) = self.cache.write()
        {
            let mut cached_bytes = cache.iter().map(|entry| entry.bytes).sum::<usize>();
            while cache.len() >= MAX_CACHE_ENTRIES || cached_bytes + bytes > MAX_CACHE_BYTES {
                if let Some(entry) = cache.pop_front() {
                    cached_bytes -= entry.bytes;
                }
            }
            cache.push_back(CacheEntry {
                syntax: syntax.name.clone(),
                key,
                lines: lines.clone(),
                bytes,
            });
        }
        Some(lines)
    }
}

fn syntax_for_path(path: &str) -> Option<&'static SyntaxReference> {
    if let Some(syntax) = language::syntax_for_filename(path) {
        return Some(syntax);
    }
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    let extension = match extension.as_str() {
        "mjs" | "cjs" | "jsx" => "js",
        "mdx" => "md",
        extension => extension,
    };
    SYNTAXES
        .find_syntax_by_extension(extension)
        .filter(|syntax| syntax.name != "Plain Text")
}

fn read_content(cwd: &Path, path: &str, revision: Option<&str>) -> Option<String> {
    validate_relative_path(path).ok()?;
    if let Some(revision) = revision {
        return run_git(
            cwd,
            &["show", &format!("{revision}:{path}")],
            &[0],
            u64::try_from(MAX_FILE_BYTES).ok()?,
        )
        .ok()
        .map(|output| output.stdout);
    }
    let path = cwd.join(path).canonicalize().ok()?;
    if !path.starts_with(cwd) || !path.is_file() {
        return None;
    }
    let mut content = String::new();
    File::open(path)
        .ok()?
        .take(u64::try_from(MAX_FILE_BYTES + 1).ok()?)
        .read_to_string(&mut content)
        .ok()?;
    (content.len() <= MAX_FILE_BYTES).then_some(content)
}

fn reconstruct(file: &ParsedDiffFile, side: DiffLineKind) -> Option<(u64, String)> {
    let mut lines = BTreeMap::new();
    for hunk in &file.hunks {
        let mut number = if side == DiffLineKind::Remove {
            hunk.old_start
        } else {
            hunk.new_start
        };
        for line in &hunk.lines {
            if line.kind == side || line.kind == DiffLineKind::Context {
                lines.insert(number, line.content.as_str());
                number = number.checked_add(1)?;
            }
        }
    }
    let first = *lines.first_key_value()?.0;
    let last = *lines.last_key_value()?.0;
    let line_count = usize::try_from(last.checked_sub(first)?).ok()?;
    let bytes = lines.values().map(|line| line.len()).sum::<usize>() + line_count;
    if bytes > MAX_FILE_BYTES {
        return None;
    }
    let mut content = String::with_capacity(bytes);
    for number in first..=last {
        if number != first {
            content.push('\n');
        }
        if let Some(line) = lines.get(&number) {
            content.push_str(line);
        }
    }
    Some((first, content))
}

fn tokenize_content(content: &str, syntax: &SyntaxReference) -> Option<Vec<Vec<HighlightToken>>> {
    let mut parser = ParseState::new(syntax);
    let mut scopes = ScopeStack::new();
    content
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let operations = parser.parse_line(line, &SYNTAXES).ok()?;
            let mut tokens: Vec<HighlightToken> = Vec::new();
            for (text, operation) in ScopeRegionIterator::new(&operations, line) {
                scopes.apply(operation).ok()?;
                if text.is_empty() {
                    continue;
                }
                let role = scopes.as_slice().iter().rev().find_map(|scope| {
                    ROLES
                        .iter()
                        .find_map(|(prefix, role)| prefix.is_prefix_of(*scope).then_some(*role))
                });
                if let Some(previous) = tokens.last_mut()
                    && previous.style.as_deref() == role
                {
                    previous.text.push_str(text);
                } else {
                    tokens.push(HighlightToken {
                        text: text.to_owned(),
                        style: role.map(str::to_owned),
                    });
                }
            }
            Some(tokens)
        })
        .collect()
}

fn apply_tokens(file: &mut ParsedDiffFile, old: Option<&TokenLookup>, new: Option<&TokenLookup>) {
    for hunk in &mut file.hunks {
        let mut old_number = hunk.old_start;
        let mut new_number = hunk.new_start;
        for line in &mut hunk.lines {
            let tokens = match line.kind {
                DiffLineKind::Header => continue,
                DiffLineKind::Add => {
                    let tokens = new.and_then(|lookup| lookup.get(new_number));
                    new_number += 1;
                    tokens
                }
                DiffLineKind::Remove => {
                    let tokens = old.and_then(|lookup| lookup.get(old_number));
                    old_number += 1;
                    tokens
                }
                DiffLineKind::Context => {
                    let tokens = new.and_then(|lookup| lookup.get(new_number));
                    old_number += 1;
                    new_number += 1;
                    tokens
                }
            };
            // A concurrent write or ignored whitespace must not replace displayed code.
            if let Some(tokens) = tokens
                && tokens_match(tokens, &line.content)
            {
                line.tokens = Some(tokens.clone());
            }
        }
    }
}

fn tokens_match(tokens: &[HighlightToken], content: &str) -> bool {
    let mut remaining = content;
    for token in tokens {
        let Some(rest) = remaining.strip_prefix(&token.text) else {
            return false;
        };
        remaining = rest;
    }
    remaining.is_empty()
}

#[cfg(test)]
mod tests;
