//! Shared validation for optional reasons attached to moderation events.
pub const MAX_REASON_BYTES: usize = 4000;

pub fn reason(input: &str) -> Result<Option<String>, String> {
    let input = input.trim();
    if input.len() > MAX_REASON_BYTES {
        return Err(format!(
            "Keep the moderation reason within {MAX_REASON_BYTES} bytes."
        ));
    }
    Ok((!input.is_empty()).then(|| input.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_reasons_are_trimmed_and_bounded_by_utf8_bytes() {
        assert_eq!(reason("  ").unwrap(), None);
        assert_eq!(reason("  Spam  ").unwrap().as_deref(), Some("Spam"));
        assert!(reason(&"a".repeat(MAX_REASON_BYTES)).is_ok());
        assert!(reason(&"é".repeat(MAX_REASON_BYTES)).is_err());
    }
}
