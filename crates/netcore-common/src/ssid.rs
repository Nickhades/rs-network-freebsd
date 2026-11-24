//! Lossless handling for 802.11 SSIDs.
//!
//! An SSID is a sequence of up to 32 arbitrary octets, not necessarily UTF-8.
//! The JSON/UI string is the exact UTF-8 text when the bytes happen to be
//! valid UTF-8 (the common case, including Chinese names). Otherwise it is an
//! unambiguous \xNN rendering, while the original octets remain available in
//! the sibling ssid_hex field.

use std::fmt::Write as _;

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// LLowercase hexadecimal representation without separators.
#[must_use]
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Decode a hexadecimal SSID. Returns None for odd lengths or non-hex input.
#[must_use]
pub fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let encoded = hex.as_bytes();
    if !encoded.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_chunks::<2>().0 {
        bytes.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Some(bytes)
}

/// Decode raw SSID octets into a display string and lossless hex.
///
/// Valid UTF-8 (including Chinese) is returned unchanged. Invalid octets are
/// represented as \xNN so the value never contains replacement characters and
/// never silently changes meaning.
#[must_use]
pub fn display_and_hex(bytes: &[u8]) -> (String, Option<String>) {
    let display = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let mut escaped = String::with_capacity(bytes.len() * 4);
            for byte in bytes {
                let _ = write!(escaped, "\\x{byte:02x}");
            }
            escaped
        }
    };
    let hex = (!bytes.is_empty()).then(|| encode_hex(bytes));
    (display, hex)
}

/// Compare two SSIDs by raw bytes when both have hex, otherwise by display.
#[must_use]
pub fn same_ssid(
    left_display: &str,
    left_hex: Option<&str>,
    right_display: &str,
    right_hex: Option<&str>,
) -> bool {
    match (left_hex, right_hex) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => left_display == right_display,
    }
}

/// Whether the hex-encoded SSID is valid UTF-8 and can be passed through the
/// string-oriented ConnectWifi API without changing bytes.
#[must_use]
pub fn is_utf8_hex(hex: &str) -> bool {
    decode_hex(hex).is_some_and(|bytes| std::str::from_utf8(&bytes).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn hex_round_trips_arbitrary_ssid(bytes in prop::collection::vec(any::<u8>(), 0..=32)) {
            let encoded = encode_hex(&bytes);
            let decoded = decode_hex(&encoded);
            prop_assert_eq!(decoded.as_deref(), Some(bytes.as_slice()));
            let (display, hex) = display_and_hex(&bytes);
            prop_assert_eq!(hex.as_deref(), (!bytes.is_empty()).then_some(encoded.as_str()));
            let replacement = '\u{FFFD}';
            prop_assert!(!display.contains(replacement));
        }
    }

    #[test]
    fn preserves_chinese_utf8() {
        let bytes = "云涧".as_bytes();
        let (display, hex) = display_and_hex(bytes);
        assert_eq!(display, "云涧");
        assert_eq!(hex.as_deref(), Some("e4ba91e6b6a7"));
        assert!(is_utf8_hex("e4ba91e6b6a7"));
    }

    #[test]
    fn escapes_invalid_octets_without_replacement() {
        let (display, hex) = display_and_hex(&[0xff, b'a', 0x00]);
        assert_eq!(display, "\\xff\\x61\\x00");
        assert_eq!(hex.as_deref(), Some("ff6100"));
        assert!(!is_utf8_hex("ff6100"));
    }

    #[test]
    fn preserves_spaces_and_empty() {
        let (display, hex) = display_and_hex(b"  net  ");
        assert_eq!(display, "  net  ");
        assert_eq!(hex.as_deref(), Some("20206e65742020"));
        assert_eq!(display_and_hex(b""), (String::new(), None));
    }

    #[test]
    fn decodes_hex_losslessly() {
        assert_eq!(
            decode_hex("e4ba91e6b6a7").as_deref(),
            Some("云涧".as_bytes())
        );
        assert_eq!(decode_hex("abc"), None);
        assert_eq!(decode_hex("zz"), None);
    }

    #[test]
    fn compares_raw_bytes_before_display() {
        assert!(same_ssid(
            "云涧",
            Some("e4ba91e6b6a7"),
            "云涧",
            Some("E4BA91E6B6A7")
        ));
        assert!(!same_ssid(
            "云涧",
            Some("e4ba91e6b6a7"),
            "云涧",
            Some("e4ba91e6b6a8")
        ));
    }
}
