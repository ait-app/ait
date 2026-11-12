//! Paseo history matching: every token may match any recalled name, including bounded typos.

#[derive(Debug)]
struct Token {
    text: String,
    units: Vec<u16>,
    edits: usize,
}

/// Query normalized once for all directory candidates.
#[derive(Debug)]
pub(super) struct Query {
    tokens: Vec<Token>,
}

impl Query {
    /// Compile whitespace-separated query tokens and their upstream typo budgets.
    pub(super) fn new(query: &str) -> Self {
        Self {
            tokens: query
                .to_lowercase()
                .split_whitespace()
                .map(|text| {
                    let units: Vec<_> = text.encode_utf16().collect();
                    let edits = match units.len() {
                        0..=3 => 0,
                        4..=7 => 1,
                        _ => 2,
                    };
                    Token {
                        text: text.to_owned(),
                        units,
                        edits,
                    }
                })
                .collect(),
        }
    }

    /// Match workspace, Agent, branch and project names without requiring tokens in one field.
    pub(super) fn matches(&self, fields: [&str; 4]) -> bool {
        if self.tokens.is_empty() {
            return true;
        }
        let fields = fields.map(str::to_lowercase);
        self.tokens
            .iter()
            .all(|token| fields.iter().any(|field| token.matches(field)))
    }
}

impl Token {
    fn matches(&self, field: &str) -> bool {
        if field.contains(&self.text) || subsequence(&self.text, field) {
            return true;
        }
        self.edits > 0
            && field
                .split(|character: char| !character.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .any(|word| {
                    let word = word.as_bytes();
                    [
                        word.len(),
                        word.len().min(self.units.len()),
                        word.len().min(self.units.len() + self.edits),
                    ]
                    .into_iter()
                    .any(|end| {
                        let candidate = &word[..end];
                        if self.units.len() == 4 {
                            transposition(&self.units, candidate)
                        } else {
                            within_edit_budget(&self.units, candidate, self.edits)
                        }
                    })
                })
    }
}

fn subsequence(query: &str, field: &str) -> bool {
    field.split_whitespace().any(|word| {
        let mut remaining = query.chars();
        let mut next = remaining.next();
        for character in word.chars() {
            if next == Some(character) {
                next = remaining.next();
                if next.is_none() {
                    return true;
                }
            }
        }
        false
    })
}

fn transposition(query: &[u16], word: &[u8]) -> bool {
    if query.len() != word.len() {
        return false;
    }
    let Some(index) = query
        .iter()
        .zip(word)
        .position(|(left, right)| *left != u16::from(*right))
    else {
        return false;
    };
    index + 1 < query.len()
        && query[index] == u16::from(word[index + 1])
        && query[index + 1] == u16::from(word[index])
        && query[index + 2..]
            .iter()
            .zip(&word[index + 2..])
            .all(|(left, right)| *left == u16::from(*right))
}

fn within_edit_budget(query: &[u16], word: &[u8], budget: usize) -> bool {
    if query.len().abs_diff(word.len()) > budget {
        return false;
    }
    let outside = budget + 1;
    let mut two_back = vec![outside; word.len() + 1];
    let mut previous: Vec<_> = (0..=word.len()).map(|index| index.min(outside)).collect();
    let mut current = vec![outside; word.len() + 1];
    for row in 1..=query.len() {
        let start = row.saturating_sub(budget).max(1);
        let end = (row + budget).min(word.len());
        current[0] = row.min(outside);
        if start > 1 {
            current[start - 1] = outside;
        }
        if end < word.len() {
            current[end + 1] = outside;
        }
        let mut best = outside;
        for column in start..=end {
            let substitution = usize::from(query[row - 1] != u16::from(word[column - 1]));
            let mut distance = (current[column - 1] + 1)
                .min(previous[column] + 1)
                .min(previous[column - 1] + substitution);
            if row > 1
                && column > 1
                && query[row - 1] == u16::from(word[column - 2])
                && query[row - 2] == u16::from(word[column - 1])
            {
                distance = distance.min(two_back[column - 2] + 1);
            }
            current[column] = distance;
            best = best.min(distance);
        }
        if best > budget {
            return false;
        }
        std::mem::swap(&mut two_back, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[word.len()] <= budget
}

#[cfg(test)]
mod tests;
