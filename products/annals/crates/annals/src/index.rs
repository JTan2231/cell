use unicode_normalization::UnicodeNormalization;

#[must_use]
pub(crate) fn normalize(text: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in text.nfkc().flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalization_is_language_stable() {
        assert_eq!(normalize("  Predicate\tLOCKING  "), "predicate locking");
        assert_eq!(normalize("Ａ"), "a");
    }
}
