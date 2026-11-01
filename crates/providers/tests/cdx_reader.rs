//! CDX reader acceptance tests over hand-assembled bytes.

mod common;

use chemifree_domain::interchange::{Content, Object, ObjectKind, PropertyKind, PropertyValue};
use chemifree_providers::cdx::{self, CdxError};
use common::*;

fn read(bytes: &[u8]) -> Result<Object, CdxError> {
    cdx::read(bytes).map(|file| file.document().root().clone())
}

fn refused(bytes: &[u8]) -> CdxError {
    read(bytes).expect_err("the file must be refused")
}

fn child_objects(object: &Object) -> Vec<&Object> {
    object
        .children
        .iter()
        .filter_map(|child| match child {
            Content::Object(object) => Some(object),
            _ => None,
        })
        .collect()
}

fn prop_value<'a>(object: &'a Object, kind: &PropertyKind) -> Option<&'a PropertyValue> {
    object
        .properties
        .iter()
        .find(|item| &item.kind == kind)
        .map(|item| &item.value)
}

/// A document with one page and one fragment holding a single node with `node_parts`.
fn with_node(node_parts: &[Vec<u8>]) -> Vec<u8> {
    file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(0x8003, 2, &[obj(0x8004, 3, node_parts)])],
        )],
    )
}

fn node_of(root: &Object) -> &Object {
    let page = child_objects(root)[0];
    let fragment = child_objects(page)[0];
    child_objects(fragment)[0]
}

#[test]
fn reads_graph_text_graphic_and_tables() {
    let root = read(&rich_file()).unwrap();
    assert_eq!(root.id, None);
    let kinds: Vec<_> = child_objects(&root)
        .iter()
        .map(|o| o.kind.clone())
        .collect();
    assert_eq!(
        kinds,
        [
            ObjectKind::FontTable,
            ObjectKind::ColorTable,
            ObjectKind::Page
        ]
    );
    let page = child_objects(&root)[2];
    let fragment = child_objects(page)[0];
    let nodes = child_objects(fragment);
    assert_eq!(
        prop_value(nodes[1], &PropertyKind::Position),
        Some(&PropertyValue::Point { x: 25.5, y: 20.0 })
    );
    assert_eq!(
        prop_value(nodes[1], &PropertyKind::Charge),
        Some(&PropertyValue::Integer(-1))
    );
    assert_eq!(
        prop_value(nodes[1], &PropertyKind::AtomicNumber),
        Some(&PropertyValue::Integer(8))
    );
    let bond = child_objects(fragment)[2];
    assert_eq!(bond.id, Some(5));
    assert_eq!(
        prop_value(bond, &PropertyKind::BondBegin),
        Some(&PropertyValue::ObjectReference(3))
    );
    let label = child_objects(page)[1];
    let runs = child_objects(label);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].children, vec![Content::Text("Caf\u{e9} ".into())]);
    assert_eq!(
        prop_value(runs[0], &PropertyKind::FontSize),
        Some(&PropertyValue::Float(10.0))
    );
    assert_eq!(
        prop_value(runs[1], &PropertyKind::FontSize),
        Some(&PropertyValue::Float(12.0))
    );
    let graphic = child_objects(page)[2];
    assert_eq!(
        prop_value(graphic, &PropertyKind::BoundingBox),
        Some(&PropertyValue::Rectangle {
            left: 1.0,
            top: 2.0,
            right: 30.0,
            bottom: 40.0
        })
    );
    let represents = child_objects(graphic)[0];
    assert_eq!(represents.kind, ObjectKind::Represents);
    assert_eq!(
        prop_value(represents, &PropertyKind::RepresentAttribute),
        Some(&PropertyValue::Text("Charge".into()))
    );
}

#[test]
fn public_documentation_coordinate_examples_decode() {
    // "1 inch (72 points) to the right, and 2 inches down: 00 00 90 00 00 00 48 00".
    let bytes = with_node(&[prop(0x0200, &[0, 0, 0x90, 0, 0, 0, 0x48, 0])]);
    assert_eq!(
        prop_value(node_of(&read(&bytes).unwrap()), &PropertyKind::Position),
        Some(&PropertyValue::Point { x: 72.0, y: 144.0 })
    );
}

#[test]
fn negative_coordinates_and_signed_values_keep_their_sign() {
    let bytes = with_node(&[
        prop(0x0200, &[&i32le(-98304)[..], &i32le(-65536)[..]].concat()),
        prop(0x0421, &[0xff]),
    ]);
    let root = read(&bytes).unwrap();
    let node = node_of(&root);
    assert_eq!(
        prop_value(node, &PropertyKind::Position),
        Some(&PropertyValue::Point { x: -1.0, y: -1.5 })
    );
    assert_eq!(
        prop_value(node, &PropertyKind::Charge),
        Some(&PropertyValue::Integer(-1))
    );
    // The same bits as an unsigned isotope would be 65535; the property is signed.
    let error = refused(&with_node(&[prop(0x0420, &i16le(-1))]));
    assert_eq!(error.code, "out_of_range");
}

#[test]
fn reserved_header_bytes_are_not_interpreted() {
    let clean = rich_file();
    let mut changed = clean.clone();
    for byte in &mut changed[8..28] {
        *byte = 0xa5;
    }
    assert_eq!(read(&changed).unwrap(), read(&clean).unwrap());
}

#[test]
fn document_id_zero_reads_as_no_id_and_other_ids_are_kept() {
    let page = obj(0x8001, 1, &[]);
    assert_eq!(
        read(&file(0, std::slice::from_ref(&page))).unwrap().id,
        None
    );
    assert_eq!(read(&file(77, &[page])).unwrap().id, Some(77));
}

#[test]
fn four_byte_charge_is_shifted_and_a_fraction_is_refused() {
    for (saved, charge) in [
        (0xff00_0000_u32, -1),
        (0x0200_0000, 2),
        (0x8000_0000, -128),
        (0x7f00_0000, 127),
        (0, 0),
    ] {
        let root = read(&with_node(&[prop(0x0421, &saved.to_le_bytes())])).unwrap();
        assert_eq!(
            prop_value(node_of(&root), &PropertyKind::Charge),
            Some(&PropertyValue::Integer(charge)),
            "saved {saved:#x}"
        );
    }
    let bytes = with_node(&[prop(0x0421, &0x0200_0001_u32.to_le_bytes())]);
    let error = refused(&bytes);
    assert_eq!(error.code, "out_of_range");
    // The property tag sits right after the node header: 28 + 6 + 6 + 6 + 6.
    assert_eq!(error.offset, Some(28 + 6 + 6 + 6 + 6));
    assert_eq!(
        refused(&with_node(&[prop(0x0421, &[1, 2, 3])])).code,
        "invalid_property"
    );
}

#[test]
fn known_enumerations_outside_their_documented_range_are_refused_with_the_offset() {
    let cases: [(u16, Vec<u8>); 9] = [
        (0x0400, i16le(14).to_vec()),
        (0x0400, i16le(-1).to_vec()),
        (0x0402, i16le(0).to_vec()),
        (0x0402, i16le(119).to_vec()),
        (0x0422, vec![4]),
        (0x0422, vec![255]),
        (0x0420, i16le(-5).to_vec()),
        (0x0402, vec![0]),
        (0x0400, vec![200]),
    ];
    for (tag, data) in cases {
        let error = refused(&with_node(&[prop(tag, &data)]));
        assert_eq!(error.code, "out_of_range", "tag {tag:#x} {data:?}");
        assert_eq!(error.offset, Some(28 + 6 + 6 + 6 + 6), "tag {tag:#x}");
    }
    for (kind_tag, data) in [(0x0A00_u16, i16le(8).to_vec()), (0x0600, u16le(0).to_vec())] {
        let owner = if kind_tag == 0x0A00 {
            obj(
                0x8007,
                4,
                &[
                    prop(0x0204, &rect(0.0, 0.0, 1.0, 1.0)),
                    prop(kind_tag, &data),
                ],
            )
        } else {
            obj(
                0x8005,
                4,
                &[
                    prop(0x0604, &u32le(3)),
                    prop(0x0605, &u32le(5)),
                    prop(kind_tag, &data),
                ],
            )
        };
        let nodes = [obj(0x8004, 3, &[]), obj(0x8004, 5, &[])];
        let fragment = obj(
            0x8003,
            2,
            &[nodes[0].clone(), nodes[1].clone(), owner.clone()],
        );
        let page = if kind_tag == 0x0A00 {
            obj(0x8001, 1, &[owner])
        } else {
            obj(0x8001, 1, &[fragment])
        };
        assert_eq!(
            refused(&file(0, &[page])).code,
            "out_of_range",
            "tag {kind_tag:#x}"
        );
    }
}

#[test]
fn enumerations_at_their_limits_are_accepted() {
    for (tag, data) in [
        (0x0400, i16le(13).to_vec()),
        (0x0402, i16le(118).to_vec()),
        (0x0422, vec![3]),
        (0x0420, i16le(i16::MAX).to_vec()),
    ] {
        read(&with_node(&[prop(tag, &data)])).unwrap_or_else(|error| panic!("{tag:#x}: {error:?}"));
    }
}

#[test]
fn unspecified_bond_order_is_kept_as_the_raw_bitset() {
    let nodes = [obj(0x8004, 3, &[]), obj(0x8004, 5, &[])];
    let bond = obj(
        0x8005,
        4,
        &[
            prop(0x0604, &u32le(3)),
            prop(0x0605, &u32le(5)),
            prop(0x0600, &u16le(0xffff)),
        ],
    );
    let page = obj(
        0x8001,
        1,
        &[obj(0x8003, 2, &[nodes[0].clone(), nodes[1].clone(), bond])],
    );
    let root = read(&file(0, &[page])).unwrap();
    let bond = child_objects(child_objects(child_objects(&root)[0])[0])[2];
    assert_eq!(
        prop_value(bond, &PropertyKind::BondOrder),
        Some(&PropertyValue::Integer(0xffff))
    );
}

#[test]
fn unstyled_text_is_latin_one_and_refuses_the_ambiguous_range() {
    let label = |text: &[u8]| {
        file(
            0,
            &[obj(
                0x8001,
                1,
                &[obj(0x8006, 2, &[prop(0x0700, &unstyled(text))])],
            )],
        )
    };
    let root = read(&label(b"Caf\xe9\r\n\t")).unwrap();
    let text = child_objects(child_objects(&root)[0])[0];
    let run = child_objects(text)[0];
    assert!(run.properties.is_empty());
    assert_eq!(run.children, vec![Content::Text("Caf\u{e9}\r\n\t".into())]);
    // Offset of the byte: header 28, document 6, page 6, text 6, property header 4, count 2.
    let base = 28 + 6 + 6 + 6 + 4 + 2;
    for (bytes, code, index) in [
        (&b"ab\x85"[..], "unsupported_text_encoding", 2),
        (b"\x80", "unsupported_text_encoding", 0),
        (b"\x9f", "unsupported_text_encoding", 0),
        (b"xx\x01", "invalid_text", 2),
        (b"\x0b", "invalid_text", 0),
        (b"a\x0c", "invalid_text", 1),
        (b"\x1f", "invalid_text", 0),
    ] {
        let error = refused(&label(bytes));
        assert_eq!(error.code, code, "{bytes:?}");
        assert_eq!(error.offset, Some(base + index), "{bytes:?}");
    }
    // 0xa0 and 0xff are ordinary Latin-1 characters.
    assert!(read(&label(b"\xa0\xff")).is_ok());
}

#[test]
fn styled_text_applies_the_same_encoding_rules() {
    let doc = |text: &[u8]| {
        file(
            0,
            &[
                prop(0x0100, &font_table(1, &[(3, 1252, "Arial")])),
                obj(
                    0x8001,
                    1,
                    &[obj(
                        0x8006,
                        2,
                        &[prop(
                            0x0700,
                            &styled(&[(3, 0, 200, 0, b"ok"), (3, 0, 200, 0, text)]),
                        )],
                    )],
                ),
            ],
        )
    };
    let font_table_len = 4 + 6 + 5;
    // document start 28+6, font table property, page 6, text 6, prop header 4, count 2, 2 runs.
    let base = 28 + 6 + 4 + font_table_len + 6 + 6 + 4 + 2 + 20 + 2;
    for (bytes, code) in [
        (&b"\x85"[..], "unsupported_text_encoding"),
        (b"\x00", "invalid_text"),
        (b"z\x08", "invalid_text"),
    ] {
        let error = refused(&doc(bytes));
        assert_eq!(error.code, code, "{bytes:?}");
        let expected = base + bytes.iter().position(|b| *b < 0x20 || *b == 0x85).unwrap() as u64;
        assert_eq!(error.offset, Some(expected), "{bytes:?}");
    }
    assert!(read(&doc(b"\xe9\t\n\r")).is_ok());
}

#[test]
fn font_charsets_and_platforms_are_never_guessed() {
    let doc = |table: Vec<u8>| file(0, &[prop(0x0100, &table), obj(0x8001, 1, &[])]);
    let table_start = 28 + 6 + 4;
    // The second entry is unused and unsupported: the whole file is refused at the entry.
    let bytes = doc(font_table(1, &[(3, 1252, "Arial"), (4, 1251, "Cyr")]));
    let error = refused(&bytes);
    assert_eq!(error.code, "unsupported_charset");
    let second_entry = table_start + 4 + 6 + 5;
    assert_eq!(error.offset, Some(second_entry as u64 + 2));
    assert_eq!(
        refused(&doc(font_table(2, &[(3, 1252, "A")]))).code,
        "invalid_font_table"
    );
    assert_eq!(refused(&doc(font_table(1, &[]))).code, "invalid_font_table");
    // A Macintosh table is readable, but only ASCII text may use its fonts.
    let mac = |text: &[u8]| {
        file(
            0,
            &[
                prop(0x0100, &font_table(0, &[(3, 1252, "Geneva")])),
                obj(
                    0x8001,
                    1,
                    &[obj(
                        0x8006,
                        2,
                        &[prop(0x0700, &styled(&[(3, 0, 200, 0, text)]))],
                    )],
                ),
            ],
        )
    };
    assert!(read(&mac(b"plain")).is_ok());
    assert_eq!(refused(&mac(b"caf\xe9")).code, "unsupported_text_encoding");
    // A truncated name inside the table points into the table.
    let mut short = font_table(1, &[(3, 1252, "Arial")]);
    short.truncate(short.len() - 2);
    assert_eq!(refused(&doc(short)).code, "invalid_font_table");
}

#[test]
fn style_runs_must_cover_the_text_in_order_with_known_fonts_and_colors() {
    let wrap = |text: Vec<u8>| {
        file(
            0,
            &[
                prop(0x0100, &font_table(1, &[(3, 1252, "Arial")])),
                obj(0x8001, 1, &[obj(0x8006, 2, &[prop(0x0700, &text)])]),
            ],
        )
    };
    let mut first_not_zero = styled(&[(3, 0, 200, 0, b"ab")]);
    first_not_zero[2] = 1;
    assert_eq!(refused(&wrap(first_not_zero)).code, "invalid_text_runs");
    let mut past_end = styled(&[(3, 0, 200, 0, b"a"), (3, 0, 200, 0, b"b")]);
    past_end[12] = 9;
    assert_eq!(refused(&wrap(past_end)).code, "invalid_text_runs");
    let mut backwards = styled(&[(3, 0, 200, 0, b"ab"), (3, 0, 200, 0, b"cd")]);
    backwards[12] = 1;
    backwards[2] = 0;
    let mut out_of_order = styled(&[
        (3, 0, 200, 0, b"ab"),
        (3, 0, 200, 0, b"c"),
        (3, 0, 200, 0, b"d"),
    ]);
    out_of_order[12] = 3;
    out_of_order[22] = 2;
    assert_eq!(refused(&wrap(out_of_order)).code, "invalid_text_runs");
    assert_eq!(
        refused(&wrap(styled(&[(9, 0, 200, 0, b"a")]))).code,
        "unknown_font"
    );
    assert_eq!(
        refused(&wrap(styled(&[(3, 0, 200, 4, b"a")]))).code,
        "unknown_color"
    );
    // Runs that claim more header than the string holds.
    assert_eq!(
        refused(&wrap(vec![0xff, 0xff, 0, 0])).code,
        "invalid_text_runs"
    );
}

#[test]
fn empty_style_runs_become_runs_without_children() {
    let doc = file(
        0,
        &[
            prop(0x0100, &font_table(1, &[(3, 1252, "Arial")])),
            obj(
                0x8001,
                1,
                &[obj(
                    0x8006,
                    2,
                    &[prop(
                        0x0700,
                        &styled(&[
                            (3, 0, 200, 0, b""),
                            (3, 1, 200, 0, b"x"),
                            (3, 0, 200, 0, b""),
                        ]),
                    )],
                )],
            ),
        ],
    );
    let root = read(&doc).unwrap();
    let text = child_objects(child_objects(&root)[1])[0];
    let runs = child_objects(text);
    assert_eq!(runs.len(), 3);
    assert!(runs[0].children.is_empty() && runs[2].children.is_empty());
    assert_eq!(runs[1].children, vec![Content::Text("x".into())]);
    let empty = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(0x8006, 2, &[prop(0x0700, &unstyled(b""))])],
        )],
    );
    let root = read(&empty).unwrap();
    let run = child_objects(child_objects(child_objects(&root)[0])[0])[0];
    assert!(run.properties.is_empty() && run.children.is_empty());
}

#[test]
fn extended_lengths_carry_long_text_and_may_spell_short_values() {
    let long: Vec<u8> = (0..70_000).map(|i| b'a' + (i % 26) as u8).collect();
    let doc = |prop_bytes: Vec<u8>| file(0, &[obj(0x8001, 1, &[obj(0x8006, 2, &[prop_bytes])])]);
    let root = read(&doc(prop(0x0700, &unstyled(&long)))).unwrap();
    let run = child_objects(child_objects(child_objects(&root)[0])[0])[0];
    let Content::Text(text) = &run.children[0] else {
        panic!("text expected")
    };
    assert_eq!(text.as_bytes(), long.as_slice());
    // Exactly 0xFFFF bytes must use the extended form, and 0xFFFE the short form.
    for length in [0xfffe_usize, 0xffff, 0x1_0000] {
        let data = unstyled(&vec![b'q'; length - 2]);
        assert_eq!(data.len(), length);
        assert!(read(&doc(prop(0x0700, &data))).is_ok(), "{length}");
    }
    // A short value in the extended spelling is accepted and decodes the same.
    let spelled = doc(prop_extended(0x0700, &unstyled(b"hi")));
    assert_eq!(
        read(&spelled).unwrap(),
        read(&doc(prop(0x0700, &unstyled(b"hi")))).unwrap()
    );
    // An extended length larger than the rest of the file is a truncation at the property.
    let mut lying = doc(prop_extended(0x0700, &unstyled(b"hi")));
    let at = lying.iter().position(|b| *b == 0xff).unwrap();
    lying[at + 2..at + 6].copy_from_slice(&u32le(0x00ff_ffff));
    let error = refused(&lying);
    assert_eq!(error.code, "truncated");
    assert_eq!(error.offset, Some(at as u64 - 2));
    // A length that would overflow a 32-bit position is still only a truncation.
    lying[at + 2..at + 6].copy_from_slice(&u32le(u32::MAX));
    assert_eq!(refused(&lying).code, "truncated");
}

#[test]
fn unknown_objects_and_properties_stay_opaque_with_their_descendants() {
    let unknown_object = obj(
        0x8030,
        20,
        &[
            prop(0x0990, &[1, 2, 3]),
            obj(0x8004, 21, &[prop(0x0402, &i16le(6))]),
        ],
    );
    let doc = file(
        0,
        &[
            prop(0x0003, b"\x00\x00ChemDraw"),
            obj(0x8001, 1, &[unknown_object]),
        ],
    );
    let root = read(&doc).unwrap();
    assert_eq!(
        prop_value(&root, &PropertyKind::UnknownBinary(3)),
        Some(&PropertyValue::Bytes(b"\x00\x00ChemDraw".to_vec()))
    );
    let opaque = child_objects(child_objects(&root)[0])[0];
    assert_eq!(opaque.kind, ObjectKind::UnknownBinary(0x8030));
    assert_eq!(opaque.id, Some(20));
    // A known tag nested in an unknown object is not interpreted.
    let inner = child_objects(opaque)[0];
    assert_eq!(inner.kind, ObjectKind::UnknownBinary(0x8004));
    assert_eq!(
        inner.properties[0].kind,
        PropertyKind::UnknownBinary(0x0402)
    );
}

#[test]
fn known_tags_on_the_wrong_object_stay_opaque_like_the_cdxml_reader() {
    // Element on a bond, and a bounding box on a node, have no meaning in the model.
    let doc = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[obj(0x8004, 3, &[prop(0x0204, &rect(0.0, 0.0, 1.0, 1.0))])],
            )],
        )],
    );
    let root = read(&doc).unwrap();
    let node = node_of(&root);
    assert_eq!(node.properties[0].kind, PropertyKind::UnknownBinary(0x0204));
}

#[test]
fn structural_errors_carry_codes_and_offsets() {
    let good = rich_file();
    let mut wrong_magic = good.clone();
    wrong_magic[0] = b'X';
    assert_eq!(
        (refused(&wrong_magic).code, refused(&wrong_magic).offset),
        ("invalid_signature", Some(0))
    );
    assert_eq!(refused(b"").code, "truncated");
    assert_eq!(refused(b"VjCD").code, "truncated");
    let mut wrong_root = good.clone();
    wrong_root[28] = 0x01;
    wrong_root[29] = 0x80;
    let error = refused(&wrong_root);
    assert_eq!((error.code, error.offset), ("invalid_root", Some(28)));
    let mut trailing = good.clone();
    trailing.push(0);
    let error = refused(&trailing);
    assert_eq!(
        (error.code, error.offset),
        ("trailing_data", Some(good.len() as u64))
    );
    let mut no_marker = good.clone();
    let end = no_marker.len();
    no_marker.truncate(end - 2);
    assert_eq!(refused(&no_marker).code, "truncated");
    let mut bad_marker = good.clone();
    let end = bad_marker.len();
    bad_marker[end - 2] = 1;
    let error = refused(&bad_marker);
    assert_eq!(
        (error.code, error.offset),
        ("invalid_end_marker", Some(end as u64 - 2))
    );
}

#[test]
fn identity_problems_are_refused_at_the_second_object() {
    let page = |id| obj(0x8001, id, &[]);
    let error = refused(&file(0, &[page(4), page(4)]));
    assert_eq!(error.code, "duplicate_id");
    assert_eq!(error.offset, Some(28 + 6 + 6 + 2));
    // Zero is not an identity, so any number of unnumbered objects is fine.
    assert!(read(&file(0, &[page(0), page(0)])).is_ok());
    let dangling = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[
                    obj(0x8004, 3, &[]),
                    obj(
                        0x8005,
                        4,
                        &[prop(0x0604, &u32le(3)), prop(0x0605, &u32le(99))],
                    ),
                ],
            )],
        )],
    );
    let error = refused(&dangling);
    assert_eq!(error.code, "dangling_reference");
    assert!(error.offset.is_some());
    let page_target = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[
                    obj(0x8004, 3, &[]),
                    obj(
                        0x8005,
                        4,
                        &[prop(0x0604, &u32le(3)), prop(0x0605, &u32le(1))],
                    ),
                ],
            )],
        )],
    );
    assert_eq!(refused(&page_target).code, "invalid_reference");
    let zero = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[
                    obj(0x8004, 3, &[]),
                    obj(
                        0x8005,
                        4,
                        &[prop(0x0604, &u32le(3)), prop(0x0605, &u32le(0))],
                    ),
                ],
            )],
        )],
    );
    assert_eq!(refused(&zero).code, "invalid_reference");
}

#[test]
fn containment_and_required_properties_are_checked_with_offsets() {
    let node_in_page = file(0, &[obj(0x8001, 1, &[obj(0x8004, 3, &[])])]);
    let error = refused(&node_in_page);
    assert_eq!(
        (error.code, error.offset),
        ("invalid_containment", Some(28 + 6 + 6))
    );
    let bond_alone = file(
        0,
        &[obj(0x8001, 1, &[obj(0x8003, 2, &[obj(0x8005, 4, &[])])])],
    );
    assert_eq!(refused(&bond_alone).code, "missing_property");
    let graphic_alone = file(0, &[obj(0x8001, 1, &[obj(0x8007, 4, &[])])]);
    assert_eq!(refused(&graphic_alone).code, "missing_property");
    let no_page = file(0, &[]);
    assert_eq!(refused(&no_page).code, "invalid_document");
    let twice = file(
        0,
        &[
            obj(0x8001, 1, &[]),
            prop(0x0100, &font_table(1, &[(1, 1252, "A")])),
            prop(0x0100, &font_table(1, &[(1, 1252, "B")])),
        ],
    );
    assert_eq!(refused(&twice).code, "duplicate_font");
    let repeated = file(
        0,
        &[
            obj(0x8001, 1, &[obj(0x8004, 3, &[])]),
            prop(0x0204, &rect(0.0, 0.0, 1.0, 1.0)),
            prop(0x0204, &rect(0.0, 0.0, 1.0, 1.0)),
        ],
    );
    assert!(matches!(
        refused(&repeated).code,
        "duplicate_property" | "invalid_containment"
    ));
}

#[test]
fn color_tables_define_the_available_indexes() {
    let doc = |table: Option<Vec<u8>>| {
        let mut parts = Vec::new();
        parts.push(prop(0x0100, &font_table(1, &[(3, 1252, "A")])));
        if let Some(table) = table {
            parts.push(prop(0x0300, &table));
        }
        parts.push(obj(
            0x8001,
            1,
            &[obj(
                0x8006,
                2,
                &[prop(0x0700, &styled(&[(3, 0, 200, 3, b"x")]))],
            )],
        ));
        file(0, &parts)
    };
    // Without a table indexes 0 to 3 exist; with one table of two colors, indexes 0 to 3 too.
    assert!(read(&doc(None)).is_ok());
    assert!(read(&doc(Some(color_table(&[[0, 0, 0], [1, 1, 1]])))).is_ok());
    assert_eq!(
        refused(&doc(Some(color_table(&[[0, 0, 0]])))).code,
        "unknown_color"
    );
    // An empty table is refused: the model requires at least one color.
    let error = refused(&doc(Some(color_table(&[]))));
    assert_eq!(error.code, "invalid_color_table");
    let mut short = color_table(&[[1, 2, 3]]);
    short.pop();
    assert_eq!(refused(&doc(Some(short))).code, "invalid_color_table");
}

#[test]
fn colors_decode_to_unit_fractions() {
    let doc = file(
        0,
        &[
            prop(0x0300, &color_table(&[[65535, 0, 32768]])),
            obj(0x8001, 1, &[]),
        ],
    );
    let root = read(&doc).unwrap();
    let table = child_objects(&root)[0];
    let color = child_objects(table)[0];
    assert_eq!(
        prop_value(color, &PropertyKind::Red),
        Some(&PropertyValue::Float(1.0))
    );
    assert_eq!(
        prop_value(color, &PropertyKind::Green),
        Some(&PropertyValue::Float(0.0))
    );
    assert_eq!(
        prop_value(color, &PropertyKind::Blue),
        Some(&PropertyValue::Float(32768.0 / 65535.0))
    );
}

#[test]
fn resource_limits_are_enforced_at_the_documented_boundary_with_an_offset() {
    // Size: one byte over 1 MiB.
    let oversized = vec![0_u8; 1024 * 1024 + 1];
    let error = refused(&oversized);
    assert_eq!(
        (error.code, error.offset),
        ("resource_limit", Some(1024 * 1024))
    );
    // Depth: the model allows depth 128 for its deepest object.
    fn nested(levels: usize) -> Vec<u8> {
        let mut inner = obj(0x8030, 0, &[]);
        for _ in 1..levels {
            inner = obj(0x8030, 0, &[inner]);
        }
        file(0, &[obj(0x8001, 1, &[]), inner])
    }
    // Root is depth 0; a chain of N unknown objects reaches depth N.
    assert!(read(&nested(128)).is_ok());
    let error = refused(&nested(129));
    assert_eq!(error.code, "resource_limit");
    // The offending object is the 129th: 28 header + document (6) + page (8) + 128 * 6.
    assert_eq!(error.offset, Some(28 + 6 + 8 + 128 * 6));
    // Object count: 99,999 groups plus the document and page reach 100,000 objects + 1.
    let groups = |count: usize| {
        let parts: Vec<Vec<u8>> = (0..count).map(|_| obj(0x8002, 0, &[])).collect();
        file(0, &[obj(0x8001, 1, &parts)])
    };
    assert!(
        read(&groups(99_998)).is_ok(),
        "document + page + 99,998 groups is 100,000 objects"
    );
    let error = refused(&groups(99_999));
    assert_eq!(error.code, "resource_limit");
    assert_eq!(error.offset, Some(28 + 6 + 6 + 99_998 * 8));
}

#[test]
fn objects_created_from_properties_count_toward_the_same_limit() {
    // 65,535 colors plus the table make 65,536 objects; with 34,000 groups the total passes
    // 100,000 and the refusal points at the color-table property, before it is expanded.
    let colors: Vec<[u16; 3]> = vec![[0, 0, 0]; 65_535];
    let groups: Vec<Vec<u8>> = (0..34_500).map(|_| obj(0x8002, 0, &[])).collect();
    let table = prop(0x0300, &color_table(&colors));
    let doc = file(0, &[table, obj(0x8001, 1, &groups)]);
    assert!(doc.len() < 1024 * 1024, "{}", doc.len());
    let error = refused(&doc);
    assert_eq!(error.code, "resource_limit");
    assert_eq!(error.offset, Some(28 + 6));
    // The same table alone is fine.
    assert!(
        read(&file(
            0,
            &[prop(0x0300, &color_table(&colors)), obj(0x8001, 1, &[])]
        ))
        .is_ok()
    );
}

#[test]
fn text_runs_that_would_exceed_the_depth_limit_are_refused() {
    // A text object at depth 128 cannot hold style runs at depth 129.
    let mut inner = obj(0x8006, 0, &[prop(0x0700, &unstyled(b"x"))]);
    // Fifty-nine unknown wrappers would not reach it, so nest known groups instead.
    for _ in 0..126 {
        inner = obj(0x8002, 0, &[inner]);
    }
    let doc = file(
        0,
        &[
            prop(0x0100, &font_table(1, &[(1, 1252, "A")])),
            obj(0x8001, 1, &[inner]),
        ],
    );
    // page depth 1, 126 groups depth 2..=127, text depth 128: its runs would be 129.
    let error = refused(&doc);
    assert_eq!(error.code, "resource_limit");
}

#[test]
fn every_truncated_prefix_of_rich_files_is_refused() {
    for original in [rich_file(), rich_file_with_opaque()] {
        assert!(read(&original).is_ok());
        for length in 0..original.len() {
            let error = refused(&original[..length]);
            assert!(
                error.offset.is_none_or(|offset| offset <= length as u64),
                "prefix {length}: offset {:?} lies beyond the data ({error:?})",
                error.offset
            );
        }
    }
}

#[test]
fn seeded_mutations_never_panic_and_accepted_results_rewrite_or_refuse_cleanly() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let seeds = [rich_file(), rich_file_with_opaque()];
    let mut accepted = 0;
    for round in 0..6_000 {
        let mut bytes = seeds[round % 2].clone();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(bytes.len());
            match rng.below(4) {
                0 => bytes[at] = rng.next() as u8,
                1 => bytes[at] ^= 1 << rng.below(8),
                2 => {
                    bytes.truncate(at.max(1));
                }
                _ => bytes.insert(at, rng.next() as u8),
            }
        }
        if let Ok(file) = cdx::read(&bytes) {
            accepted += 1;
            // Whatever was accepted must also be writable (or refused with a code), never a panic.
            match cdx::write(file.document(), Some(&file)) {
                Ok(written) => assert!(cdx::read(&written.bytes).is_ok(), "round {round}"),
                Err(error) => assert!(!error.code.is_empty()),
            }
        }
    }
    assert!(
        accepted > 100,
        "the sweep should also reach accepted files, got {accepted}"
    );
}

#[test]
fn repeated_font_table_or_represents_properties_are_only_allowed_where_they_mean_something() {
    let table = prop(0x0100, &font_table(1, &[(1, 1252, "A")]));
    // Two font tables on a page, and two Represents on a node, are plain duplicates.
    let page = file(0, &[obj(0x8001, 1, &[table.clone(), table.clone()])]);
    let error = refused(&page);
    assert_eq!(error.code, "duplicate_property");
    // Header 28 + document 6 + page 6 + first property (4 + 7 + 6).
    assert_eq!(error.offset, Some(28 + 6 + 6 + 4 + table.len() as u64 - 4));
    let represents = prop(0x000e, &[&u32le(1)[..], &u16le(0x0421)[..]].concat());
    let node = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(
                0x8003,
                2,
                &[obj(0x8004, 3, &[represents.clone(), represents])],
            )],
        )],
    );
    assert_eq!(refused(&node).code, "duplicate_property");
}

#[test]
fn a_text_object_without_its_string_is_refused_at_the_object() {
    let doc = file(
        0,
        &[obj(
            0x8001,
            1,
            &[obj(0x8006, 2, &[prop(0x0200, &point(1.0, 2.0))])],
        )],
    );
    let error = refused(&doc);
    assert_eq!(error.code, "missing_property");
    assert_eq!(error.offset, Some(28 + 6 + 6));
}

#[test]
fn a_represents_property_at_the_depth_limit_is_refused_at_the_property() {
    let represents = prop(0x000e, &[&u32le(1)[..], &u16le(0x0421)[..]].concat());
    let mut inner = obj(
        0x8007,
        0,
        &[prop(0x0204, &rect(0.0, 0.0, 1.0, 1.0)), represents],
    );
    for _ in 0..126 {
        inner = obj(0x8002, 0, &[inner]);
    }
    // Page depth 1, 126 groups at depths 2..=127, the graphic at depth 128.
    let doc = file(0, &[obj(0x8001, 1, &[inner])]);
    let error = refused(&doc);
    assert_eq!(error.code, "resource_limit");
    assert!(error.offset.is_some(), "{error:?}");
}
