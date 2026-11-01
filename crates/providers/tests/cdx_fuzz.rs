//! Deterministic truncation and mutation sweep over a CDX file with every supported feature.
use chemifree_providers::{cdx, cdxml};

fn begin(bytes: &mut Vec<u8>, tag: u16, id: u32) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend(id.to_le_bytes());
}
fn field(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
    bytes.extend(tag.to_le_bytes());
    bytes.extend((value.len() as u16).to_le_bytes());
    bytes.extend(value);
}
fn base() -> Vec<u8> {
    let mut data = b"VjCD0100\x04\x03\x02\x01".to_vec();
    data.extend([0; 16]);
    let mut fonts = Vec::new();
    for value in [1u16, 1, 3, 1252, 5] {
        fonts.extend(value.to_le_bytes());
    }
    fonts.extend(b"Arial");
    field(&mut data, 0x0100, &fonts);
    let mut colors = 2u16.to_le_bytes().to_vec();
    for value in [0u16, 0, 0, 65535, 32768, 1] {
        colors.extend(value.to_le_bytes());
    }
    field(&mut data, 0x0300, &colors);
    begin(&mut data, 0x8001, 1);
    begin(&mut data, 0x8003, 2);
    for (id, x, element) in [(3u32, -2i32, 8i16), (4, 3, 6)] {
        begin(&mut data, 0x8004, id);
        let mut xy = (x * 65536).to_le_bytes().to_vec();
        xy.extend(65536i32.to_le_bytes());
        field(&mut data, 0x0200, &xy);
        field(&mut data, 0x0402, &element.to_le_bytes());
        if id == 3 {
            field(&mut data, 0x0421, &[0xff]);
            field(&mut data, 0x0422, &[2]);
        }
        data.extend([0, 0]);
    }
    begin(&mut data, 0x8005, 5);
    field(&mut data, 0x0604, &3u32.to_le_bytes());
    field(&mut data, 0x0605, &4u32.to_le_bytes());
    field(&mut data, 0x0600, &2u16.to_le_bytes());
    data.extend([0, 0]);
    data.extend([0, 0]);
    begin(&mut data, 0x8006, 6);
    let mut text = Vec::new();
    for value in [2u16, 0, 3, 1, 240, 2, 2, 3, 2, 240, 2] {
        text.extend(value.to_le_bytes());
    }
    text.extend(b"Test");
    field(&mut data, 0x0700, &text);
    data.extend([0, 0]);
    data.extend([0, 0]);
    data.extend([0, 0]);
    data
}

fn through_cdxml(
    document: &chemifree_domain::interchange::Document,
    label: &str,
) -> Option<String> {
    match cdxml::write(&cdxml::CdxmlDocument::from_document(document.clone())) {
        Ok(xml) => Some(xml),
        // Unknown binary objects have no CDXML spelling; everything else must transcode.
        Err(error) if error.code == "unsafe_unknown_rewrite" => None,
        Err(error) => panic!("{label}: CDXML write: {error:?}"),
    }
}

fn check_round_trips(bytes: &[u8], label: &str) {
    let Ok(first) = cdx::read(bytes) else { return };
    // Every accepted CDX must read back identically after a trip through CDXML.
    if let Some(xml) = through_cdxml(&first.document, label) {
        let reread = cdxml::read(&xml)
            .unwrap_or_else(|error| panic!("{label}: CDXML reread: {error:?}\n{xml}"));
        assert_eq!(
            reread.document().root(),
            first.document.root(),
            "{label}: CDX tree vs CDXML tree\n{xml}"
        );
    }
    let Ok(written) = cdx::write(&first.document) else {
        return;
    };
    let second = cdx::read(&written.bytes)
        .unwrap_or_else(|error| panic!("{label}: rewritten CDX unreadable: {error}"));
    assert_eq!(
        second.document.root(),
        first.document.root(),
        "{label}: read-write-read"
    );
    let xml = through_cdxml(&first.document, label).expect("a writable tree has no opaque binary");
    let reread = cdxml::read(&xml).unwrap();
    let back = cdx::write(reread.document())
        .unwrap_or_else(|error| panic!("{label}: CDX from CDXML: {error:?}\n{xml}"));
    let final_read = cdx::read(&back.bytes).unwrap();
    assert_eq!(
        final_read.document.root(),
        first.document.root(),
        "{label}: CDX-CDXML-CDX\n{xml}"
    );
}

#[test]
fn the_base_file_round_trips_and_every_proper_prefix_is_refused() {
    let bytes = base();
    let read = cdx::read(&bytes).expect("base file is valid");
    assert_eq!(cdx::write(&read.document).unwrap().bytes, bytes);
    check_round_trips(&bytes, "base");
    for cut in 0..bytes.len() {
        assert!(
            cdx::read(&bytes[..cut]).is_err(),
            "prefix of {cut} bytes was accepted"
        );
    }
}

#[test]
fn seeded_mutations_never_panic_and_keep_round_trip_identity() {
    let bytes = base();
    // CDX_FUZZ_SEED and CDX_FUZZ_ROUNDS widen the sweep when hunting; defaults are deterministic.
    let env = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let mut state = env("CDX_FUZZ_SEED", 0x9e37_79b9_7f4a_7c15);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..env("CDX_FUZZ_ROUNDS", 30_000) {
        let mut mutated = bytes.clone();
        for _ in 0..=(next() % 3) {
            let at = (next() as usize) % mutated.len();
            match next() % 4 {
                0 => mutated[at] ^= 1 << (next() % 8),
                1 => mutated[at] = next() as u8,
                2 => {
                    mutated.remove(at);
                }
                _ => mutated.insert(at, next() as u8),
            }
        }
        check_round_trips(&mutated, &format!("round {round}: {mutated:02x?}"));
    }
}

#[test]
fn cdxml_fixtures_agree_after_a_trip_through_cdx() {
    let mut sources: Vec<String> = vec![
        "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 -2\" Element=\"8\" Charge=\"-1\" Radical=\"Doublet\"/><n id=\"4\" p=\"3 4\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"2\"/></fragment></page></CDXML>".into(),
        "<CDXML><fonttable><font id=\"3\" name=\"Arial\" charset=\"iso-8859-1\"/></fonttable><colortable><color r=\"0\" g=\"0\" b=\"0\"/><color r=\"1\" g=\"1\" b=\"1\"/></colortable><page id=\"1\"><t id=\"2\" p=\"1 2\"><s font=\"3\" face=\"1\" size=\"12\" color=\"1\">Te</s><s font=\"3\" face=\"2\" size=\"10.5\" color=\"0\">st</s></t><t id=\"6\"><s>plain</s></t><t id=\"7\"><s/></t></page></CDXML>".into(),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/cdxml");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "cdxml")
        {
            sources.push(std::fs::read_to_string(path).unwrap());
        }
    }
    for source in &sources {
        let xml = cdxml::read(source).unwrap();
        let Ok(binary) = cdx::write(xml.document()) else {
            continue;
        };
        let from_binary = cdx::read(&binary.bytes).expect("written CDX must be readable");
        let again = cdxml::write(&cdxml::CdxmlDocument::from_document(
            from_binary.document.clone(),
        ))
        .and_then(|text| cdxml::read(&text))
        .unwrap();
        assert_eq!(
            again.document().root(),
            from_binary.document.root(),
            "{source}"
        );
        assert_eq!(
            from_binary.document.root(),
            xml.document().root(),
            "CDXML to CDX changed the tree: {source}"
        );
    }
}

#[test]
fn seeded_cdxml_mutations_never_write_cdx_the_reader_refuses() {
    let base = "<CDXML><fonttable><font id=\"3\" name=\"Arial\" charset=\"iso-8859-1\"/></fonttable><colortable><color r=\"0\" g=\"0\" b=\"0\"/><color r=\"1\" g=\"0.5\" b=\"1\"/></colortable><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 -2\" Element=\"8\" Charge=\"-1\" Isotope=\"18\" Radical=\"Doublet\"/><n id=\"4\" p=\"3 4\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"2\"/></fragment><t id=\"6\" p=\"1 2\"><s font=\"3\" face=\"1\" size=\"12\" color=\"1\">Te</s><s font=\"3\" face=\"2\" size=\"10.5\" color=\"0\">st</s></t><t id=\"7\"><s>caf\u{e9}</s></t></page></CDXML>".as_bytes().to_vec();
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let rounds = std::env::var("CDX_FUZZ_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000);
    let mut exercised = 0;
    for round in 0..rounds {
        let mut mutated = base.clone();
        for _ in 0..=(next() % 3) {
            let at = (next() as usize) % mutated.len();
            match next() % 3 {
                0 => mutated[at] = b"0123456789-. <>\"/&;#x"[(next() as usize) % 21],
                1 => {
                    mutated.remove(at);
                }
                _ => mutated.insert(at, b"0123456789-. "[(next() as usize) % 13]),
            }
        }
        let Ok(text) = String::from_utf8(mutated) else {
            continue;
        };
        let Ok(xml) = cdxml::read(&text) else {
            continue;
        };
        let Ok(written) = cdx::write(xml.document()) else {
            continue;
        };
        let read = cdx::read(&written.bytes).unwrap_or_else(|error| {
            panic!("round {round}: CDX written but unreadable: {error:?}\n{text}")
        });
        if written.rounded_color_channels == 0 {
            assert_eq!(
                read.document.root(),
                xml.document().root(),
                "round {round}: {text}"
            );
        }
    }
}
