//! CDX strings, font tables and color tables.
//!
//! A CDX string is single-byte text. The public documentation says that a string without
//! style runs is ISO Latin 1 and that a font's character set selects the encoding of the runs
//! that use it, but it lists no character-set values beyond the example pairing code page
//! 1252 with the CDXML name `iso-8859-1`. That is the only pairing accepted here. Bytes
//! 0x80 to 0x9f are controls in Latin 1 and punctuation in Windows-1252, so they are
//! refused rather than guessed, in styled and unstyled text alike. Text of a Macintosh-platform
//! font is accepted only as ASCII, because the Macintosh meaning of the upper half is not
//! established by the public pages.

use std::collections::HashMap;

use super::CdxError;
use crate::cdxml::is_xml_char;

/// Windows code page number written for the supported character set.
pub(super) const CHARSET_WINDOWS_1252: u16 = 1252;
/// CDXML spelling paired with that code page by the public font-table example.
pub(super) const CHARSET_NAME: &str = "iso-8859-1";

pub(super) fn charset_name(code: u16) -> Option<&'static str> {
    (code == CHARSET_WINDOWS_1252).then_some(CHARSET_NAME)
}

pub(super) fn charset_code(name: &str) -> Option<u16> {
    name.eq_ignore_ascii_case(CHARSET_NAME)
        .then_some(CHARSET_WINDOWS_1252)
}

/// Platform value of a font table: 0 Macintosh, 1 Windows.
pub(super) const PLATFORM_WINDOWS: u16 = 1;

#[derive(Clone, Copy)]
pub(super) struct FontInfo {
    pub platform: u16,
}

pub(super) struct FontEntry {
    pub id: u16,
    pub charset: u16,
    pub name: String,
    /// Position of this entry's first byte.
    pub offset: u64,
}

pub(super) struct FontTable {
    pub platform: u16,
    pub entries: Vec<FontEntry>,
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

/// Decodes single-byte text. `ascii_only` is set for Macintosh-platform fonts.
pub(super) fn decode_text(bytes: &[u8], base: u64, ascii_only: bool) -> Result<String, CdxError> {
    let mut text = String::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        let at = base + index as u64;
        let character = char::from(*byte);
        match *byte {
            0x80..=0x9f => {
                return Err(CdxError::at(
                    "unsupported_text_encoding",
                    at,
                    format!(
                        "Byte 0x{byte:02x} is a control in Latin 1 and punctuation in Windows-1252; the text encoding is not guessed."
                    ),
                ));
            }
            0xa0..=0xff if ascii_only => {
                return Err(CdxError::at(
                    "unsupported_text_encoding",
                    at,
                    format!(
                        "Byte 0x{byte:02x} in text of a Macintosh-platform font has no established meaning."
                    ),
                ));
            }
            _ if !is_xml_char(character) => {
                return Err(CdxError::at(
                    "invalid_text",
                    at,
                    format!("Byte 0x{byte:02x} is a control character that XML 1.0 cannot carry."),
                ));
            }
            _ => text.push(character),
        }
    }
    Ok(text)
}

/// Encodes text for a Windows-1252/Latin-1 string, or explains the first impossible character.
pub(super) fn encode_text(text: &str, out: &mut Vec<u8>) -> Result<(), String> {
    for character in text.chars() {
        let code = u32::from(character);
        let representable =
            code <= 0xff && !(0x80..=0x9f).contains(&code) && is_xml_char(character);
        if !representable {
            return Err(format!(
                "U+{code:04X} cannot be written in a CDX string: only ISO Latin 1 without U+0080 to U+009F is supported."
            ));
        }
        out.push(code as u8);
    }
    Ok(())
}

/// Parses a CDXFontTable value found at `base` in the file.
pub(super) fn parse_font_table(data: &[u8], base: u64) -> Result<FontTable, CdxError> {
    let short = |at: usize| {
        CdxError::at(
            "invalid_font_table",
            base + at as u64,
            "The font table ends inside an entry.",
        )
    };
    let platform = le16(data, 0).ok_or_else(|| short(0))?;
    if platform > 1 {
        return Err(CdxError::at(
            "invalid_font_table",
            base,
            format!(
                "The font table platform is {platform}; only 0 (Macintosh) and 1 (Windows) are defined."
            ),
        ));
    }
    let count = usize::from(le16(data, 2).ok_or_else(|| short(2))?);
    if count == 0 {
        return Err(CdxError::at(
            "invalid_font_table",
            base,
            "A font table needs at least one font.",
        ));
    }
    let mut position = 4;
    let mut entries = Vec::new();
    for _ in 0..count {
        let id = le16(data, position).ok_or_else(|| short(position))?;
        let charset = le16(data, position + 2).ok_or_else(|| short(position))?;
        let length = usize::from(le16(data, position + 4).ok_or_else(|| short(position))?);
        let name_start = position + 6;
        let name_end = name_start
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| short(position))?;
        let entry_offset = base + position as u64;
        if charset_name(charset).is_none() {
            return Err(CdxError::at(
                "unsupported_charset",
                entry_offset + 2,
                format!(
                    "Font {id} uses character set {charset}; only {CHARSET_WINDOWS_1252} (Windows-1252, CDXML \"{CHARSET_NAME}\") is supported. The file is refused even if the font is unused."
                ),
            ));
        }
        let name = decode_text(
            &data[name_start..name_end],
            base + name_start as u64,
            platform == 0,
        )?;
        entries.push(FontEntry {
            id,
            charset,
            name,
            offset: entry_offset,
        });
        position = name_end;
    }
    if position != data.len() {
        return Err(CdxError::at(
            "invalid_font_table",
            base + position as u64,
            "Bytes follow the last font entry.",
        ));
    }
    Ok(FontTable { platform, entries })
}

/// Parses a CDXColorTable value into 16-bit components.
pub(super) fn parse_color_table(data: &[u8], base: u64) -> Result<Vec<[u16; 3]>, CdxError> {
    let count = le16(data, 0).map(usize::from).ok_or_else(|| {
        CdxError::at("invalid_color_table", base, "The color table has no count.")
    })?;
    if count == 0 {
        return Err(CdxError::at(
            "invalid_color_table",
            base,
            "An empty color table is refused: it would remove the default color indexes without defining others.",
        ));
    }
    if data.len() != 2 + 6 * count {
        return Err(CdxError::at(
            "invalid_color_table",
            base,
            format!(
                "The color table declares {count} colors, which need {} bytes, but holds {}.",
                2 + 6 * count,
                data.len()
            ),
        ));
    }
    Ok((0..count)
        .map(|index| {
            let at = 2 + 6 * index;
            [
                le16(data, at).unwrap_or(0),
                le16(data, at + 2).unwrap_or(0),
                le16(data, at + 4).unwrap_or(0),
            ]
        })
        .collect())
}

/// A style run as stored: 8 bytes after the start index.
pub(super) struct Run {
    pub offset: u64,
    pub font: u16,
    pub face: u16,
    pub size: u16,
    pub color: u16,
    pub text: String,
}

pub(super) enum DecodedString {
    /// No style runs: the whole string is Latin 1 with no font.
    Unstyled(String),
    Styled(Vec<Run>),
}

/// Decodes a CDXString value found at `base`.
pub(super) fn parse_string(
    data: &[u8],
    base: u64,
    fonts: &HashMap<u16, FontInfo>,
) -> Result<DecodedString, CdxError> {
    let runs =
        usize::from(le16(data, 0).ok_or_else(|| {
            CdxError::at("invalid_text", base, "The string has no style-run count.")
        })?);
    if runs == 0 {
        return decode_text(&data[2..], base + 2, false).map(DecodedString::Unstyled);
    }
    let header_end = runs
        .checked_mul(10)
        .and_then(|bytes| bytes.checked_add(2))
        .filter(|end| *end <= data.len())
        .ok_or_else(|| {
            CdxError::at(
                "invalid_text_runs",
                base,
                format!(
                    "{runs} style runs do not fit in the {} byte string.",
                    data.len()
                ),
            )
        })?;
    let body = &data[header_end..];
    let mut starts = Vec::with_capacity(runs);
    for index in 0..runs {
        let at = 2 + 10 * index;
        let start = usize::from(le16(data, at).unwrap_or(0));
        let run_offset = base + at as u64;
        let previous = starts.last().copied().unwrap_or(0);
        if (index == 0 && start != 0) || start < previous || start > body.len() {
            return Err(CdxError::at(
                "invalid_text_runs",
                run_offset,
                format!(
                    "Style run {index} starts at character {start}, which is not within the text in order (the first run must start at 0)."
                ),
            ));
        }
        starts.push(start);
    }
    let mut result = Vec::with_capacity(runs);
    for index in 0..runs {
        let at = 2 + 10 * index;
        let run_offset = base + at as u64;
        let font = le16(data, at + 2).unwrap_or(0);
        let info = fonts.get(&font).ok_or_else(|| {
            CdxError::at(
                "unknown_font",
                run_offset + 2,
                format!("Style run {index} uses font {font}, which is not in a font table."),
            )
        })?;
        let end = starts.get(index + 1).copied().unwrap_or(body.len());
        let text = decode_text(
            &body[starts[index]..end],
            base + header_end as u64 + starts[index] as u64,
            info.platform == 0,
        )?;
        result.push(Run {
            offset: run_offset,
            font,
            face: le16(data, at + 4).unwrap_or(0),
            size: le16(data, at + 6).unwrap_or(0),
            color: le16(data, at + 8).unwrap_or(0),
            text,
        });
    }
    Ok(DecodedString::Styled(result))
}
