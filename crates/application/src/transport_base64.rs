//! Canonical standard-alphabet base64 for binary interchange payloads in JSON.
//!
//! The encoding of a byte string is unique: decoding refuses missing or misplaced padding,
//! non-zero trailing bits, whitespace and every character outside the alphabet. A preview
//! digest that binds the source text therefore binds exactly one byte string.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A refused base64 string with the character index of the offending item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Base64Error {
    pub index: usize,
    pub reason: &'static str,
}

/// The longest base64 text that can encode `bytes` bytes.
pub fn encoded_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// Encodes with `=` padding.
pub fn encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(encoded_len(bytes.len()));
    for chunk in bytes.chunks(3) {
        let group = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let value = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for position in 0..4 {
            if position <= chunk.len() {
                let sextet = (value >> (18 - 6 * position)) & 0x3f;
                output.push(char::from(ALPHABET[sextet as usize]));
            } else {
                output.push('=');
            }
        }
    }
    output
}

fn sextet(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes canonical base64.
///
/// # Errors
/// Returns the index of the first character that makes the text non-canonical.
pub fn decode(text: &str) -> Result<Vec<u8>, Base64Error> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(Base64Error {
            index: bytes.len(),
            reason: "the length is not a multiple of four",
        });
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (group_index, group) in bytes.chunks(4).enumerate() {
        let base = group_index * 4;
        let last = group_index + 1 == groups;
        let padding = group.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return Err(Base64Error {
                index: base + (4 - padding.min(4)),
                reason: "padding is only allowed as one or two final characters",
            });
        }
        let mut value = 0_u32;
        for (position, byte) in group[..4 - padding].iter().enumerate() {
            let Some(bits) = sextet(*byte) else {
                return Err(Base64Error {
                    index: base + position,
                    reason: "the character is outside the standard alphabet",
                });
            };
            value = value << 6 | bits;
        }
        value <<= 6 * padding;
        let produced = 3 - padding;
        // Unused low bits must be zero, otherwise two texts would decode to one string.
        let unused = if padding == 0 { 0 } else { 8 * padding as u32 };
        if value & ((1 << unused) - 1) != 0 {
            return Err(Base64Error {
                index: base + 3 - padding,
                reason: "the unused trailing bits are not zero",
            });
        }
        output.extend_from_slice(&value.to_be_bytes()[1..1 + produced]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc_4648_vectors() {
        for (raw, text) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(raw.as_bytes()), text);
            assert_eq!(decode(text).unwrap(), raw.as_bytes(), "{text}");
        }
    }

    #[test]
    fn round_trips_every_byte_value_and_length() {
        let all: Vec<u8> = (0..=255).collect();
        for end in 0..=all.len() {
            assert_eq!(decode(&encode(&all[..end])).unwrap(), &all[..end]);
        }
    }

    #[test]
    fn refuses_non_canonical_text() {
        for (text, index) in [
            ("Zg", 2),
            ("Zg=", 3),
            ("Zh==", 1),
            ("Zm9=", 2),
            ("Z===", 1),
            ("Zg==Zm9v", 2),
            ("Zm9v\n", 5),
            ("Zm 9", 2),
            ("Zm9-", 3),
            ("====", 0),
        ] {
            let error = decode(text).expect_err(text);
            assert_eq!(error.index, index, "{text:?}: {}", error.reason);
        }
    }
}
