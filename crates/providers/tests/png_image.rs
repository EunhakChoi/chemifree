//! Real PNG/base64 validation, including complete compressed stream and late chunks.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chemifree_domain::image::{MAX_PNG_BYTES, PngDecoder};
use chemifree_providers::png_image::EmbeddedPngDecoder;
use std::io::Write;
fn chunk(name: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend(name);
    bytes.extend(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(name);
    crc.update(data);
    bytes.extend(crc.finalize().to_be_bytes());
    bytes
}
fn image(width: u32, height: u32, rgba: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(if rgba {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![
                0;
                width as usize
                    * height as usize
                    * if rgba { 4 } else { 3 }
            ])
            .unwrap();
        writer.finish().unwrap();
    }
    bytes
}
fn parts(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut at = 8;
    let mut out = Vec::new();
    while at < bytes.len() {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        out.push((
            bytes[at + 4..at + 8].try_into().unwrap(),
            bytes[at + 8..at + 8 + size].to_vec(),
        ));
        at += size + 12;
    }
    out
}
fn assemble(parts: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    for (name, data) in parts {
        bytes.extend(chunk(name, data));
    }
    bytes
}
fn validate(bytes: &[u8]) -> Result<chemifree_domain::image::PngMetadata, String> {
    EmbeddedPngDecoder.validate(&STANDARD.encode(bytes))
}
fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn rgb_rgba_transparency_scalar_metadata_and_pixel_boundaries_decode_without_changing_source() {
    for (width, height, rgba) in [
        (1, 1, false),
        (1, 1, true),
        (2048, 512, false),
        (1024, 1024, true),
    ] {
        let bytes = image(width, height, rgba);
        let data = STANDARD.encode(&bytes);
        let meta = EmbeddedPngDecoder.validate(&data).unwrap();
        assert_eq!(
            (meta.width, meta.height, meta.compressed_bytes),
            (width, height, bytes.len())
        );
        assert_eq!(STANDARD.decode(data).unwrap(), bytes);
    }
    let mut fields = parts(&image(2, 3, true));
    fields.insert(1, (*b"sRGB", vec![0]));
    fields.insert(2, (*b"gAMA", 45455u32.to_be_bytes().to_vec()));
    fields.insert(3, (*b"pHYs", vec![0, 0, 0, 0, 0, 0, 0, 0, 0]));
    assert_eq!(validate(&assemble(&fields)).unwrap().width, 2);
}
#[test]
fn canonical_base64_and_exact_compressed_byte_limit_are_enforced() {
    let fields = parts(&image(1, 1, false));
    let mut data = vec![0x78, 0x01];
    for _ in 0..4 {
        data.extend([0, 0, 0, 255, 255]);
    }
    data.extend([1, 4, 0, 251, 255, 0, 0, 0, 0, 0, 4, 0, 1]);
    let mut fields = vec![(fields[0].0, fields[0].1.clone()), (*b"IDAT", data)];
    for _ in 0..10915 {
        fields.push((*b"IDAT", Vec::new()));
    }
    fields.push((*b"IEND", Vec::new()));
    let bytes = assemble(&fields);
    assert_eq!(bytes.len(), MAX_PNG_BYTES);
    assert_eq!(validate(&bytes).unwrap().compressed_bytes, MAX_PNG_BYTES);
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(validate(&extra).unwrap_err().contains("128 KiB"));
    let valid = STANDARD.encode(&bytes);
    for text in [
        format!("{valid}\n"),
        valid.trim_end_matches('=').into(),
        "Zg===".into(),
        "Zh==".into(),
        "A".repeat(174765),
    ] {
        assert!(
            EmbeddedPngDecoder.validate(&text).is_err(),
            "base64 form must be refused: {}bytes",
            text.len()
        );
    }
}
#[test]
fn crc_all_chunks_iend_eof_and_late_apng_are_checked_before_decoding() {
    let original = image(1, 1, true);
    let fields = parts(&original);
    for name in [*b"acTL", *b"fcTL", *b"fdAT"] {
        let mut altered = fields.clone();
        let index = altered.len() - 1;
        altered.insert(index, (name, vec![0; 8]));
        assert!(
            validate(&assemble(&altered))
                .unwrap_err()
                .contains("Animated PNG")
        );
    }
    let mut corrupt = original.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(validate(&corrupt).unwrap_err().contains("CRC"));
    for bytes in [
        original[..original.len() - 1].to_vec(),
        original[..original.len() - 12].to_vec(),
        [original.as_slice(), &[0]].concat(),
    ] {
        assert!(validate(&bytes).is_err());
    }
    for name in [*b"tEXt", *b"iCCP", *b"eXIf", *b"tRNS", *b"PLTE", *b"zzZZ"] {
        let mut altered = fields.clone();
        altered.insert(1, (name, vec![0; 4]));
        assert!(
            validate(&assemble(&altered))
                .unwrap_err()
                .contains("outside the embedded profile")
        );
    }
}
#[test]
fn unsupported_profiles_and_overlarge_headers_refuse_before_output_allocation() {
    let original = parts(&image(1, 1, false));
    for (width, height, depth, color, interlace) in [
        (2049, 1, 8, 2, 0),
        (2048, 513, 8, 2, 0),
        (1, 1, 16, 2, 0),
        (1, 1, 8, 0, 0),
        (1, 1, 8, 3, 0),
        (1, 1, 8, 6, 1),
    ] {
        let mut fields = original.clone();
        let header = &mut fields[0].1;
        header[..4].copy_from_slice(&(width as u32).to_be_bytes());
        header[4..8].copy_from_slice(&(height as u32).to_be_bytes());
        header[8] = depth;
        header[9] = color;
        header[12] = interlace;
        assert!(
            validate(&assemble(&fields)).is_err(),
            "profile {width}x{height}/{depth}/{color}/{interlace}"
        );
    }
}
#[test]
fn truncated_extra_idat_bad_adler_bad_filter_and_expansion_are_real_failures() {
    let original = parts(&image(1, 1, false));
    let ihdr = original[0].clone();
    let idat = original
        .iter()
        .filter(|(name, _)| name == b"IDAT")
        .flat_map(|(_, data)| data.clone())
        .collect::<Vec<_>>();
    let mut truncated = idat.clone();
    truncated.pop();
    let mut trailing = idat.clone();
    trailing.extend([0, 1, 2]);
    let mut checksum = idat.clone();
    let n = checksum.len();
    checksum[n - 1] ^= 1;
    for data in [
        truncated,
        trailing,
        checksum,
        zlib(&[0; 100000]),
        zlib(&[5, 0, 0, 0]),
    ] {
        let bytes = assemble(&[ihdr.clone(), (*b"IDAT", data), (*b"IEND", Vec::new())]);
        assert!(
            validate(&bytes).is_err(),
            "full raster/zlib validation must refuse malformed data"
        );
    }
    let extra = assemble(&[
        ihdr.clone(),
        (*b"IDAT", idat.clone()),
        (*b"IDAT", idat.clone()),
        (*b"IEND", Vec::new()),
    ]);
    assert!(
        validate(&extra)
            .unwrap_err()
            .contains("trailing compressed")
    );
    let split = assemble(&[
        ihdr,
        (*b"IDAT", idat[..3].to_vec()),
        (*b"IDAT", idat[3..].to_vec()),
        (*b"IEND", Vec::new()),
    ]);
    assert_eq!(validate(&split).unwrap().width, 1);
}

#[test]
fn srgb_gamma_metadata_must_agree_in_either_chunk_order() {
    for reverse in [false, true] {
        let mut fields = parts(&image(1, 1, true));
        let srgb = (*b"sRGB", vec![0]);
        let gamma = (*b"gAMA", 100000u32.to_be_bytes().to_vec());
        let metadata = if reverse {
            [gamma, srgb]
        } else {
            [srgb, gamma]
        };
        fields.splice(1..1, metadata);
        assert!(
            validate(&assemble(&fields))
                .unwrap_err()
                .contains("sRGB/gAMA")
        );
    }
}
