//! Decode Git's C-style quoted paths independently of terminal presentation settings.

use std::borrow::Cow;

pub(super) fn decode(value: &str) -> Cow<'_, str> {
    let Some(quoted) = value
        .strip_prefix('"')
        .and_then(|path| path.strip_suffix('"'))
    else {
        return Cow::Borrowed(value);
    };
    let mut decoded = Vec::with_capacity(quoted.len());
    let mut bytes = quoted.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        let Some(escaped) = bytes.next() else {
            return Cow::Borrowed(value);
        };
        let byte = match escaped {
            b'0'..=b'7' => {
                let mut number = u16::from(escaped - b'0');
                for _ in 0..2 {
                    let Some(digit) = bytes.next_if(|digit| matches!(digit, b'0'..=b'7')) else {
                        break;
                    };
                    number = number * 8 + u16::from(digit - b'0');
                }
                let Ok(byte) = u8::try_from(number) else {
                    return Cow::Borrowed(value);
                };
                byte
            }
            b'a' => 7,
            b'b' => 8,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 11,
            b'f' => 12,
            b'r' => b'\r',
            b'"' => b'"',
            b'\\' => b'\\',
            _ => return Cow::Borrowed(value),
        };
        decoded.push(byte);
    }
    String::from_utf8(decoded).map_or(Cow::Borrowed(value), Cow::Owned)
}

#[cfg(test)]
mod tests;
