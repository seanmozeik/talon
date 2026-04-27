//! NFD Unicode normalization helper.
//!
//! NFD is preferred over NFC because Apple's HFS+ (and APFS) filesystem stores
//! filenames in NFD by default, while Linux ext4/btrfs typically stores them in
//! NFC. Lowercasing after NFD decomposition ensures that NFC-encoded queries
//! and NFD-encoded vault content (or vice versa) compare equal.
//!
//! Ported from OHS `searcher.ts:115` and `db.ts:13`.

use unicode_normalization::UnicodeNormalization;

/// Returns the NFD-normalized form of `input`.
///
/// Apply this before any lowercase or comparison operation on user input or
/// vault data so that NFC and NFD representations of the same character are
/// treated as identical.
#[must_use]
pub fn normalize(input: &str) -> String {
    input.nfd().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NFC `é` (U+00E9, one code point) and NFD `é` (e + U+0301, two code
    /// points) must both normalize to the same NFD string.
    #[test]
    fn nfc_and_nfd_e_acute_compare_equal() {
        let nfc = "\u{00E9}"; // é precomposed
        let nfd = "e\u{0301}"; // e + combining acute accent
        assert_eq!(normalize(nfc), normalize(nfd));
    }

    /// Lowercased NFC and NFD forms must also be equal (the typical use-case).
    #[test]
    fn lowercased_nfc_and_nfd_compare_equal() {
        let nfc = "\u{00E9}";
        let nfd = "e\u{0301}";
        assert_eq!(normalize(nfc).to_lowercase(), normalize(nfd).to_lowercase());
    }

    /// Cyrillic text round-trips through NFD without corruption.
    #[test]
    fn cyrillic_round_trips() {
        let s = "Кириллица";
        assert_eq!(normalize(&normalize(s)), normalize(s));
    }

    /// ASCII input passes through unchanged.
    #[test]
    fn ascii_passthrough() {
        let s = "hello world";
        assert_eq!(normalize(s), s);
    }

    /// Empty input returns an empty string.
    #[test]
    fn empty_string() {
        assert_eq!(normalize(""), "");
    }
}
