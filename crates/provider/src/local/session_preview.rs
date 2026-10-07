//! Small, whitespace-normalized user-text previews for native discovery.

pub(super) fn text<'a>(parts: impl Iterator<Item = &'a str>) -> Option<String> {
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
