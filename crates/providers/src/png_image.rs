//! Bounded embedded PNG validation. Scalar metadata is retained in original bytes; no pixels
//! or colour interpretation enter the chemical graph. Unsupported PNG profiles are refused.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chemifree_domain::image::{
    MAX_BASE64_BYTES, MAX_IMAGE_EDGE, MAX_IMAGE_PIXELS, MAX_PNG_BYTES, PngDecoder, PngMetadata,
};
use std::io::Cursor;
const DECODER_BYTES: usize = 8 * 1024 * 1024;
const OUTPUT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy, Default)]
pub struct EmbeddedPngDecoder;
impl PngDecoder for EmbeddedPngDecoder {
    fn validate(&self, text: &str) -> Result<PngMetadata, String> {
        if text.len() > MAX_BASE64_BYTES {
            return Err("Embedded PNG exceeds compressed-byte policy".into());
        }
        let bytes = STANDARD
            .decode(text)
            .map_err(|_| "Embedded PNG requires canonical padded base64".to_string())?;
        if bytes.len() > MAX_PNG_BYTES || STANDARD.encode(&bytes) != text {
            return Err("Embedded PNG requires canonical base64 and at most 128 KiB".into());
        }
        let (width, height, channels) = scan(&bytes)?;
        // Calculate the complete caller-owned output allocation before invoking the decoder.
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(channels))
            .filter(|n| *n <= OUTPUT_BYTES)
            .ok_or("PNG output exceeds 4 MiB")?;
        let decoder = png::Decoder::new_with_limits(
            Cursor::new(&bytes),
            png::Limits {
                bytes: DECODER_BYTES,
            },
        );
        let mut reader = decoder
            .read_info()
            .map_err(|e| format!("Invalid PNG: {e}"))?;
        if reader.output_buffer_size() != Some(expected) {
            return Err("PNG output size disagrees with the bounded header".into());
        }
        let mut output = vec![0; expected];
        let frame = reader
            .next_frame(&mut output)
            .map_err(|e| format!("Invalid PNG frame: {e}"))?;
        if frame.width != width
            || frame.height != height
            || frame.buffer_size() != expected
            || frame.bit_depth != png::BitDepth::Eight
            || !matches!(frame.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
        {
            return Err("PNG output profile disagrees with its header".into());
        }
        reader
            .finish()
            .map_err(|e| format!("Invalid PNG tail: {e}"))?;
        Ok(PngMetadata {
            width,
            height,
            compressed_bytes: bytes.len(),
        })
    }
}
fn integer(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
fn scan(bytes: &[u8]) -> Result<(u32, u32, usize), String> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Embedded source is not PNG".into());
    }
    let mut at = 8usize;
    let mut header = None;
    let mut idat = false;
    let mut payload_bytes = 0usize;
    let mut metadata = [false; 3];
    let mut gamma = None;
    let mut idat_chunks = Vec::new();
    while at < bytes.len() {
        let fixed = bytes.get(at..at + 8).ok_or("Truncated PNG chunk header")?;
        let length = integer(&fixed[..4]) as usize;
        if length > 0x7fff_ffff {
            return Err("PNG chunk length exceeds its format limit".into());
        }
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= bytes.len())
            .ok_or("Truncated PNG chunk")?;
        let name = &fixed[4..8];
        if !name.iter().all(u8::is_ascii_alphabetic) || name[2] & 32 != 0 {
            return Err("Invalid PNG chunk type".into());
        }
        let data = &bytes[at + 8..end - 4];
        let mut crc = crc32fast::Hasher::new();
        crc.update(name);
        crc.update(data);
        if crc.finalize() != integer(&bytes[end - 4..end]) {
            return Err("PNG chunk CRC mismatch".into());
        }
        if matches!(name, b"acTL" | b"fcTL" | b"fdAT") {
            return Err("Animated PNG is outside the embedded static profile".into());
        }
        if header.is_none() && name != b"IHDR" {
            return Err("PNG requires IHDR first".into());
        }
        match name {
            b"IHDR" => {
                if header.is_some() || length != 13 {
                    return Err("PNG requires exactly one 13-byte IHDR".into());
                }
                let width = integer(&data[..4]);
                let height = integer(&data[4..8]);
                if width == 0
                    || height == 0
                    || width > MAX_IMAGE_EDGE
                    || height > MAX_IMAGE_EDGE
                    || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
                {
                    return Err("PNG dimensions exceed the embedded pixel policy".into());
                }
                if data[8] != 8
                    || !matches!(data[9], 2 | 6)
                    || data[10] != 0
                    || data[11] != 0
                    || data[12] != 0
                {
                    return Err("Only non-interlaced 8-bit RGB/RGBA PNG is supported".into());
                }
                header = Some((width, height, if data[9] == 2 { 3 } else { 4 }));
            }
            b"IDAT" => {
                idat = true;
                payload_bytes += length;
                idat_chunks.push(data);
            }
            b"IEND" => {
                if length != 0 || !idat || payload_bytes == 0 || end != bytes.len() {
                    return Err("PNG requires image data and a final empty IEND at EOF".into());
                }
                let info = header.ok_or("Missing PNG header")?;
                let expected = (info.0 as usize * info.2 + 1) * info.1 as usize;
                exact_zlib(&idat_chunks, expected)?;
                return Ok(info);
            }
            b"sRGB" | b"gAMA" | b"pHYs" => {
                let index = if name == b"sRGB" {
                    0
                } else if name == b"gAMA" {
                    1
                } else {
                    2
                };
                let valid = match name {
                    b"sRGB" => length == 1 && data[0] <= 3,
                    b"gAMA" => length == 4 && integer(data) > 0,
                    _ => length == 9 && data[8] <= 1,
                };
                if idat || metadata[index] || !valid {
                    return Err("Invalid or misplaced PNG scalar metadata".into());
                }
                metadata[index] = true;
                if index == 1 {
                    gamma = Some(integer(data));
                }
                // PNG's sRGB compatibility gAMA value is normative, independent of order.
                if metadata[0] && gamma.is_some_and(|value| value != 45455) {
                    return Err("PNG sRGB/gAMA metadata values disagree".into());
                }
            }
            _ => {
                return Err(format!(
                    "PNG chunk {} is outside the embedded profile",
                    String::from_utf8_lossy(name)
                ));
            }
        }
        at = end;
    }
    Err("PNG is missing IEND".into())
}

// png::Reader::finish consumes through IEND. Independently pin the zlib stream's
// exact row-byte count, checksum and end position with bounded scratch before raster decode.
fn exact_zlib(chunks: &[&[u8]], expected: usize) -> Result<(), String> {
    let mut decoder = flate2::Decompress::new(true);
    let mut scratch = [0; 8192];
    let mut ended = false;
    for chunk in chunks {
        let mut input = *chunk;
        while !input.is_empty() {
            if ended {
                return Err("PNG has trailing compressed IDAT data".into());
            }
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let status = decoder
                .decompress(input, &mut scratch, flate2::FlushDecompress::None)
                .map_err(|e| format!("Invalid PNG zlib stream: {e}"))?;
            let consumed = (decoder.total_in() - before_in) as usize;
            let produced = decoder.total_out() - before_out;
            if decoder.total_out() > expected as u64 {
                return Err("PNG expands beyond its exact row-byte size".into());
            }
            input = &input[consumed..];
            ended = status == flate2::Status::StreamEnd;
            if consumed == 0 && produced == 0 {
                return Err("PNG zlib stream made no progress".into());
            }
        }
    }
    while !ended {
        let before = decoder.total_out();
        let status = decoder
            .decompress(&[], &mut scratch, flate2::FlushDecompress::Finish)
            .map_err(|e| format!("Incomplete PNG zlib stream: {e}"))?;
        if decoder.total_out() > expected as u64 {
            return Err("PNG expands beyond its exact row-byte size".into());
        }
        ended = status == flate2::Status::StreamEnd;
        if !ended && decoder.total_out() == before {
            return Err("Incomplete PNG zlib stream".into());
        }
    }
    if decoder.total_out() != expected as u64 {
        return Err("PNG row data is truncated".into());
    }
    Ok(())
}
