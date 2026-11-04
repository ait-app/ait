//! Operating-system randomness rendered as the lowercase hexadecimal tokens metadata persists.

/// Render `N` random bytes as `2 * N` lowercase hexadecimal characters.
///
/// # Errors
/// Returns the operating-system random source failure.
pub(crate) fn random_hex<const N: usize>() -> Result<String, getrandom::Error> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes)?;
    let mut encoded = String::with_capacity(N * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

/// Render 16 random bytes in the 8-4-4-4-12 hyphenated layout without UUID version bits.
///
/// # Errors
/// Returns the operating-system random source failure.
pub(crate) fn random_hyphenated_id() -> Result<String, getrandom::Error> {
    let encoded = random_hex::<16>()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &encoded[0..8],
        &encoded[8..12],
        &encoded[12..16],
        &encoded[16..20],
        &encoded[20..32]
    ))
}

#[cfg(test)]
mod tests;
