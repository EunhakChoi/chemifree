//! CDX and CDXML agree on the common model: seeded round trips in both directions.

mod common;

use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, Property, PropertyKind, PropertyValue,
};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use common::Rng;

const ALPHABET: [&str; 14] = [
    "a", "B", " ", "0", "\u{e9}", "\t", "&", "<", ">", "\"", "'", "\u{a0}", "\r", "x",
];

struct Generator {
    rng: Rng,
    next_id: u32,
    node_ids: Vec<u32>,
    fonts: Vec<u16>,
    colors: u16,
}

fn property(kind: PropertyKind, value: PropertyValue) -> Property {
    Property { kind, value }
}

impl Generator {
    fn id(&mut self, always: bool) -> Option<u32> {
        if !always && self.rng.below(4) == 0 {
            return None;
        }
        self.next_id += 1;
        Some(self.next_id)
    }

    fn coordinate(&mut self) -> f64 {
        (self.rng.below(2_000_000) as f64 - 1_000_000.0) / 64.0
    }

    fn rectangle(&mut self) -> PropertyValue {
        PropertyValue::Rectangle {
            left: self.coordinate(),
            top: self.coordinate(),
            right: self.coordinate(),
            bottom: self.coordinate(),
        }
    }

    fn object(kind: ObjectKind, id: Option<u32>, children: Vec<Content>) -> Object {
        Object {
            kind,
            id,
            properties: Vec::new(),
            children,
        }
    }

    fn tables(&mut self) -> Vec<Content> {
        let mut result = Vec::new();
        let count = 1 + self.rng.below(3);
        let mut fonts = Vec::new();
        while fonts.len() < count {
            let id = self.rng.below(60) as u16;
            if !self.fonts.contains(&id) {
                self.fonts.push(id);
                let mut font = Self::object(ObjectKind::Font, None, Vec::new());
                font.properties = vec![
                    property(
                        PropertyKind::FontIndex,
                        PropertyValue::Integer(i64::from(id)),
                    ),
                    property(
                        PropertyKind::FontCharset,
                        PropertyValue::Text("iso-8859-1".into()),
                    ),
                    property(
                        PropertyKind::FontName,
                        PropertyValue::Text(format!("Font {id} \u{e9}")),
                    ),
                ];
                fonts.push(Content::Object(font));
            }
        }
        result.push(Content::Object(Self::object(
            ObjectKind::FontTable,
            None,
            fonts,
        )));
        if self.rng.below(2) == 0 {
            self.colors = 1 + self.rng.below(4) as u16;
            let colors = (0..self.colors)
                .map(|_| {
                    let mut color = Self::object(ObjectKind::Color, None, Vec::new());
                    for kind in [PropertyKind::Red, PropertyKind::Green, PropertyKind::Blue] {
                        let step = self.rng.below(65536) as f64;
                        color
                            .properties
                            .push(property(kind, PropertyValue::Float(step / 65535.0)));
                    }
                    Content::Object(color)
                })
                .collect();
            let table = Self::object(ObjectKind::ColorTable, None, colors);
            if self.rng.below(2) == 0 {
                result.insert(0, Content::Object(table));
            } else {
                result.push(Content::Object(table));
            }
        }
        result
    }

    fn text(&mut self) -> Content {
        let mut text = Self::object(ObjectKind::Text, self.id_value(), Vec::new());
        if self.rng.below(2) == 0 {
            let x = self.coordinate();
            let y = self.coordinate();
            text.properties.push(property(
                PropertyKind::Position,
                PropertyValue::Point { x, y },
            ));
        }
        if self.rng.below(3) == 0 {
            let value = self.rectangle();
            text.properties
                .push(property(PropertyKind::BoundingBox, value));
        }
        let piece = |generator: &mut Generator| -> Vec<Content> {
            let length = generator.rng.below(6);
            let string: String = (0..length)
                .map(|_| ALPHABET[generator.rng.below(ALPHABET.len())])
                .collect();
            if string.is_empty() {
                Vec::new()
            } else {
                vec![Content::Text(string)]
            }
        };
        if self.rng.below(3) == 0 {
            let children = piece(self);
            text.children.push(Content::Object(Self::object(
                ObjectKind::StyleRun,
                None,
                children,
            )));
        } else {
            for _ in 0..1 + self.rng.below(4) {
                let font = self.fonts[self.rng.below(self.fonts.len())];
                let available = if self.colors == 0 { 4 } else { 2 + self.colors };
                let mut run = Self::object(ObjectKind::StyleRun, None, piece(self));
                run.properties = vec![
                    property(
                        PropertyKind::FontIndex,
                        PropertyValue::Integer(i64::from(font)),
                    ),
                    property(
                        PropertyKind::FontSize,
                        PropertyValue::Float((1 + self.rng.below(800)) as f64 / 20.0),
                    ),
                    property(
                        PropertyKind::FontFace,
                        PropertyValue::Integer(self.rng.below(128) as i64),
                    ),
                    property(
                        PropertyKind::ColorIndex,
                        PropertyValue::Integer(self.rng.below(usize::from(available)) as i64),
                    ),
                ];
                text.children.push(Content::Object(run));
            }
        }
        Content::Object(text)
    }

    fn id_value(&mut self) -> Option<u32> {
        self.id(false)
    }

    fn fragment(&mut self) -> Content {
        let nodes = 1 + self.rng.below(6);
        let mut children = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..nodes {
            let id = self.id(true).unwrap();
            ids.push(id);
            self.node_ids.push(id);
            let mut node = Self::object(ObjectKind::Node, Some(id), Vec::new());
            if self.rng.below(4) != 0 {
                let x = self.coordinate();
                let y = self.coordinate();
                node.properties.push(property(
                    PropertyKind::Position,
                    PropertyValue::Point { x, y },
                ));
            }
            if self.rng.below(3) == 0 {
                node.properties.push(property(
                    PropertyKind::NodeType,
                    PropertyValue::Integer(self.rng.below(14) as i64),
                ));
            }
            if self.rng.below(2) == 0 {
                node.properties.push(property(
                    PropertyKind::AtomicNumber,
                    PropertyValue::Integer(1 + self.rng.below(118) as i64),
                ));
            }
            if self.rng.below(4) == 0 {
                node.properties.push(property(
                    PropertyKind::Isotope,
                    PropertyValue::Integer(self.rng.below(300) as i64),
                ));
            }
            if self.rng.below(3) == 0 {
                node.properties.push(property(
                    PropertyKind::Charge,
                    PropertyValue::Integer(self.rng.below(256) as i64 - 128),
                ));
            }
            if self.rng.below(4) == 0 {
                node.properties.push(property(
                    PropertyKind::Radical,
                    PropertyValue::Integer(self.rng.below(4) as i64),
                ));
            }
            children.push(Content::Object(node));
        }
        if ids.len() > 1 {
            for _ in 0..self.rng.below(ids.len()) {
                let first = self.rng.below(ids.len());
                let second = (first + 1 + self.rng.below(ids.len() - 1)) % ids.len();
                let mut bond = Self::object(ObjectKind::Bond, self.id_value(), Vec::new());
                bond.properties = vec![
                    property(
                        PropertyKind::BondBegin,
                        PropertyValue::ObjectReference(ids[first]),
                    ),
                    property(
                        PropertyKind::BondEnd,
                        PropertyValue::ObjectReference(ids[second]),
                    ),
                ];
                if self.rng.below(2) == 0 {
                    bond.properties.push(property(
                        PropertyKind::BondOrder,
                        PropertyValue::Integer(1 + self.rng.below(0xfffe) as i64),
                    ));
                }
                children.push(Content::Object(bond));
            }
        }
        let mut fragment = Self::object(ObjectKind::Fragment, self.id_value(), children);
        if self.rng.below(3) == 0 {
            let value = self.rectangle();
            fragment
                .properties
                .push(property(PropertyKind::BoundingBox, value));
        }
        Content::Object(fragment)
    }

    fn graphic(&mut self) -> Content {
        let mut graphic = Self::object(ObjectKind::Graphic, self.id_value(), Vec::new());
        let bounds = self.rectangle();
        graphic.properties = vec![
            property(
                PropertyKind::GraphicType,
                PropertyValue::Integer(self.rng.below(8) as i64),
            ),
            property(PropertyKind::BoundingBox, bounds),
        ];
        if let Some(target) = self.node_ids.first().copied()
            && self.rng.below(2) == 0
        {
            let name = [
                "Charge", "Isotope", "Radical", "Element", "p", "NodeType", "Order",
            ][self.rng.below(7)];
            let mut represents = Self::object(ObjectKind::Represents, None, Vec::new());
            represents.properties = vec![
                property(
                    PropertyKind::RepresentObject,
                    PropertyValue::ObjectReference(target),
                ),
                property(
                    PropertyKind::RepresentAttribute,
                    PropertyValue::Text(name.into()),
                ),
            ];
            graphic.children.push(Content::Object(represents));
        }
        Content::Object(graphic)
    }

    fn container(&mut self, depth: usize, kind: ObjectKind) -> Content {
        let mut children = Vec::new();
        for _ in 0..1 + self.rng.below(4) {
            children.push(match self.rng.below(if depth < 3 { 5 } else { 4 }) {
                0 | 1 => self.fragment(),
                2 => self.text(),
                3 => self.graphic(),
                _ => self.container(depth + 1, ObjectKind::Group),
            });
        }
        let mut object = Self::object(kind, self.id_value(), children);
        if self.rng.below(3) == 0 {
            let value = self.rectangle();
            object
                .properties
                .push(property(PropertyKind::BoundingBox, value));
        }
        Content::Object(object)
    }
}

fn generate(seed: u64) -> Document {
    let mut generator = Generator {
        rng: Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1),
        next_id: 0,
        node_ids: Vec::new(),
        fonts: Vec::new(),
        colors: 0,
    };
    let mut children = generator.tables();
    for _ in 0..1 + generator.rng.below(3) {
        children.push(generator.container(0, ObjectKind::Page));
    }
    let id = generator.id_value();
    let mut root = Generator::object(ObjectKind::Document, id, children);
    if generator.rng.below(3) == 0 {
        let value = generator.rectangle();
        root.properties
            .push(property(PropertyKind::BoundingBox, value));
    }
    Document::new(root).expect("generated trees are valid")
}

fn codec_write(format: InterchangeFormat, document: &Document, original: Option<&[u8]>) -> Vec<u8> {
    LocalInterchangeCodec
        .write(format, document, original, false)
        .unwrap_or_else(|error| panic!("{format:?}: {error:?}"))
        .bytes
}

fn codec_read(format: InterchangeFormat, bytes: &[u8]) -> Document {
    LocalInterchangeCodec
        .read(format, bytes)
        .unwrap_or_else(|error| panic!("{format:?}: {error:?}"))
        .document
}

#[test]
fn seeded_documents_round_trip_in_each_format_and_agree_across_formats() {
    for seed in 1..=400_u64 {
        let document = generate(seed);
        let cdx = codec_write(InterchangeFormat::Cdx, &document, None);
        let from_cdx = codec_read(InterchangeFormat::Cdx, &cdx);
        assert_eq!(from_cdx, document, "seed {seed}: CDX read(write(tree))");
        assert_eq!(
            codec_write(InterchangeFormat::Cdx, &from_cdx, Some(&cdx)),
            cdx,
            "seed {seed}: CDX bytes are stable"
        );
        let cdxml = codec_write(InterchangeFormat::Cdxml, &document, None);
        let from_cdxml = codec_read(InterchangeFormat::Cdxml, &cdxml);
        assert_eq!(from_cdxml, document, "seed {seed}: CDXML read(write(tree))");
        // CDX -> CDXML -> CDX.
        let through_xml = codec_read(
            InterchangeFormat::Cdxml,
            &codec_write(InterchangeFormat::Cdxml, &from_cdx, None),
        );
        assert_eq!(through_xml, from_cdx, "seed {seed}: CDX -> CDXML tree");
        assert_eq!(
            codec_write(InterchangeFormat::Cdx, &through_xml, None),
            cdx,
            "seed {seed}: CDX -> CDXML -> CDX bytes"
        );
        // CDXML -> CDX -> CDXML.
        let through_binary = codec_read(
            InterchangeFormat::Cdx,
            &codec_write(InterchangeFormat::Cdx, &from_cdxml, None),
        );
        assert_eq!(through_binary, from_cdxml, "seed {seed}: CDXML -> CDX tree");
        assert_eq!(
            codec_write(InterchangeFormat::Cdxml, &through_binary, None),
            cdxml,
            "seed {seed}: CDXML -> CDX -> CDXML text"
        );
    }
}

#[test]
fn every_prefix_and_seeded_mutation_of_generated_files_is_handled_without_panic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for seed in 1..=40_u64 {
        let document = generate(seed);
        let bytes = codec_write(InterchangeFormat::Cdx, &document, None);
        let step = (bytes.len() / 400).max(1);
        for length in (0..bytes.len()).step_by(step) {
            assert!(
                LocalInterchangeCodec
                    .read(InterchangeFormat::Cdx, &bytes[..length])
                    .is_err(),
                "seed {seed}: prefix {length} of {} accepted",
                bytes.len()
            );
        }
        assert!(
            LocalInterchangeCodec
                .read(InterchangeFormat::Cdx, &bytes[..bytes.len() - 1])
                .is_err()
        );
        for _ in 0..150 {
            let mut changed = bytes.clone();
            for _ in 0..1 + rng.below(3) {
                let at = rng.below(changed.len());
                changed[at] = rng.next() as u8;
            }
            if let Ok(read) = LocalInterchangeCodec.read(InterchangeFormat::Cdx, &changed) {
                // An accepted mutation is a valid tree: the writer takes it or refuses by code.
                let _ =
                    LocalInterchangeCodec.write(InterchangeFormat::Cdx, &read.document, None, true);
                let _ = LocalInterchangeCodec.write(
                    InterchangeFormat::Cdxml,
                    &read.document,
                    None,
                    true,
                );
            }
        }
    }
}
