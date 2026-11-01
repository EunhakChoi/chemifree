//! Hand-assembled CDX bytes. These helpers follow the public byte layout directly and share
//! no code with the codec under test.
#![allow(dead_code, reason = "each test target uses a different subset")]

pub const HEADER: [u8; 28] = {
    let mut header = [0; 28];
    let magic = *b"VjCD0100";
    let mut index = 0;
    while index < 8 {
        header[index] = magic[index];
        index += 1;
    }
    header[8] = 4;
    header[9] = 3;
    header[10] = 2;
    header[11] = 1;
    header
};

/// A property: tag, 16-bit length (or 0xFFFF plus a 32-bit length), data.
pub fn prop(tag: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_le_bytes().to_vec();
    if data.len() >= 0xffff {
        bytes.extend_from_slice(&0xffff_u16.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    } else {
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    }
    bytes.extend_from_slice(data);
    bytes
}

/// A property that always uses the extended length spelling.
pub fn prop_extended(tag: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0xffff_u16.to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

/// An object: tag, ID, parts, end marker.
pub fn obj(tag: u16, id: u32, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = tag.to_le_bytes().to_vec();
    bytes.extend_from_slice(&id.to_le_bytes());
    for part in parts {
        bytes.extend_from_slice(part);
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// Header, the document object, and the end-of-file marker.
pub fn file(document_id: u32, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(&obj(0x8000, document_id, parts));
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

pub fn i16le(value: i16) -> [u8; 2] {
    value.to_le_bytes()
}

pub fn u16le(value: u16) -> [u8; 2] {
    value.to_le_bytes()
}

pub fn i32le(value: i32) -> [u8; 4] {
    value.to_le_bytes()
}

pub fn u32le(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// CDXPoint2D: y then x, each in 1/65536 point.
pub fn point(x: f64, y: f64) -> Vec<u8> {
    let mut bytes = ((y * 65536.0) as i32).to_le_bytes().to_vec();
    bytes.extend_from_slice(&((x * 65536.0) as i32).to_le_bytes());
    bytes
}

/// CDXRectangle: top, left, bottom, right.
pub fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [top, left, bottom, right] {
        bytes.extend_from_slice(&((value * 65536.0) as i32).to_le_bytes());
    }
    bytes
}

/// A font table: platform, count, then id/charset/name-length/name per font.
pub fn font_table(platform: u16, fonts: &[(u16, u16, &str)]) -> Vec<u8> {
    let mut bytes = platform.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(fonts.len() as u16).to_le_bytes());
    for (id, charset, name) in fonts {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&charset.to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes
}

/// A color table of 16-bit triples.
pub fn color_table(colors: &[[u16; 3]]) -> Vec<u8> {
    let mut bytes = (colors.len() as u16).to_le_bytes().to_vec();
    for color in colors {
        for component in color {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// A styled string: runs of (font, face, size in 20ths, color, text).
pub fn styled(runs: &[(u16, u16, u16, u16, &[u8])]) -> Vec<u8> {
    let mut bytes = (runs.len() as u16).to_le_bytes().to_vec();
    let mut start = 0_u16;
    for (font, face, size, color, text) in runs {
        bytes.extend_from_slice(&start.to_le_bytes());
        for value in [font, face, size, color] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        start += text.len() as u16;
    }
    for (.., text) in runs {
        bytes.extend_from_slice(text);
    }
    bytes
}

/// An unstyled string: a zero run count then the bytes.
pub fn unstyled(text: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(text);
    bytes
}

/// Deterministic xorshift generator for seeded sweeps.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// A representative file: font and color tables, a page with a molecule, a styled label,
/// a graphic and an unknown property and object. Returns the bytes.
pub fn rich_file() -> Vec<u8> {
    rich(false)
}

/// The same file with an unknown property (CreationProgram) on the document.
pub fn rich_file_with_opaque() -> Vec<u8> {
    rich(true)
}

fn rich(with_opaque: bool) -> Vec<u8> {
    let node = |id, x, y, element: i16, charge: i8| {
        obj(
            0x8004,
            id,
            &[
                prop(0x0200, &point(x, y)),
                prop(0x0402, &i16le(element)),
                prop(0x0421, &charge.to_le_bytes()),
            ],
        )
    };
    let fragment = obj(
        0x8003,
        2,
        &[
            node(3, 10.0, 20.0, 6, 0),
            node(4, 25.5, 20.0, 8, -1),
            obj(
                0x8005,
                5,
                &[
                    prop(0x0604, &u32le(3)),
                    prop(0x0605, &u32le(4)),
                    prop(0x0600, &u16le(2)),
                ],
            ),
        ],
    );
    let label = obj(
        0x8006,
        6,
        &[
            prop(0x0200, &point(40.0, 50.0)),
            prop(
                0x0700,
                &styled(&[(3, 1, 200, 2, b"Caf\xe9 "), (4, 0, 240, 3, b"H2O")]),
            ),
        ],
    );
    let graphic = obj(
        0x8007,
        7,
        &[
            prop(0x0204, &rect(1.0, 2.0, 30.0, 40.0)),
            prop(0x0A00, &i16le(1)),
            prop(0x000e, &[&u32le(4)[..], &u16le(0x0421)[..]].concat()),
        ],
    );
    let page = obj(0x8001, 1, &[fragment, label, graphic]);
    let creation = if with_opaque {
        prop(0x0003, b"\x00\x00Test program")
    } else {
        Vec::new()
    };
    file(
        0,
        &[
            creation,
            prop(
                0x0100,
                &font_table(1, &[(3, 1252, "Arial"), (4, 1252, "Times New Roman")]),
            ),
            prop(
                0x0300,
                &color_table(&[[65535, 65535, 65535], [0, 0, 0], [65535, 0, 0]]),
            ),
            page,
        ],
    )
}
