//! Small, whitespace-normalized user-text previews for native discovery.

/// Join `parts` into a whitespace-normalized preview of at most 300 characters.
///
/// Returns `None` when the parts contain no visible text.
pub fn text<'a>(parts: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut preview = String::new();
    let mut remaining = 300;
    for word in parts.flat_map(str::split_whitespace) {
        if !preview.is_empty() {
            if remaining == 0 {
                break;
            }
            preview.push(' ');
            remaining -= 1;
        }
        for character in word.chars().take(remaining) {
            preview.push(character);
            remaining -= 1;
        }
        if remaining == 0 {
            break;
        }
    }
    (!preview.is_empty()).then_some(preview)
}

#[cfg(test)]
mod tests;
