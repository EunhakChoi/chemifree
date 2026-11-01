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

fn check_round_trips(bytes: &[u8], label: &str) {
    let Ok(first) = cdx::read(bytes) else { return };
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
    // CDX to CDXML to CDX must keep the tree whenever CDX can represent it.
    let xml = cdxml::write(&cdxml::CdxmlDocument::from_document(first.document.clone()))
        .unwrap_or_else(|error| panic!("{label}: CDXML write: {error:?}"));
    let reread =
        cdxml::read(&xml).unwrap_or_else(|error| panic!("{label}: CDXML reread: {error:?}\n{xml}"));
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
