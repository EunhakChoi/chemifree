//! The stereo statements of a CDXML or CDX fragment, read from the opaque properties the rich
//! tree keeps for them. Only what the public sources confirm is read: the bond `Display` and
//! `Display2` values and the atom `Geometry`, `BondOrdering`, `AS`, `EnhancedStereoType` and
//! `EnhancedStereoGroupNum` attributes in CDXML, and the atom tags `0430`, `0431`, `0446` and
//! `0447` (and `0437` as a hint) in CDX, and the binary bond `Display` (`0601`, an `INT16`), which is decoded through
//! the same `LOOKS` table as the CDXML names. `Display2` and `DoublePosition` have no tag in the
//! public sources used, so a binary bond that carries one stays an unread property and is
//! refused, exactly as before.

use chemifree_domain::interchange::{Object, ObjectKind, PropertyKind, PropertyValue, XmlName};
use chemifree_domain::stereo::{BondEnd, StereoMark};
use chemifree_domain::stereo_group::GroupKind;

use super::fail;
use crate::interchange_codec::InterchangeError;
use crate::{Loss, LossConsent, LossDisposition, SemanticImpact};

/// CDX `Geometry` (tag `0430`) byte for tetrahedral.
const CDX_GEOMETRY_TETRAHEDRAL: u8 = 7;
/// Most neighbour entries a `BondOrdering` may list (a tetrahedral center has four).
const MAX_ORDERING_ENTRIES: usize = 16;

/// A stereo-bearing property a node or bond may carry without making the projection refuse it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Carrier {
    Geometry,
    BondOrdering,
    Label,
    EnhancedType,
    EnhancedNumber,
    Display,
    Display2,
}

fn carrier(owner: &ObjectKind, kind: &PropertyKind) -> Option<Carrier> {
    match (owner, kind) {
        (
            ObjectKind::Node,
            PropertyKind::UnknownXml(XmlName {
                namespace: None,
                local,
            }),
        ) => match local.as_str() {
            "Geometry" => Some(Carrier::Geometry),
            "BondOrdering" => Some(Carrier::BondOrdering),
            "AS" => Some(Carrier::Label),
            "EnhancedStereoType" => Some(Carrier::EnhancedType),
            "EnhancedStereoGroupNum" => Some(Carrier::EnhancedNumber),
            _ => None,
        },
        (ObjectKind::Node, PropertyKind::UnknownBinary(tag)) => match tag {
            0x0430 => Some(Carrier::Geometry),
            0x0431 => Some(Carrier::BondOrdering),
            0x0437 => Some(Carrier::Label),
            0x0446 => Some(Carrier::EnhancedType),
            0x0447 => Some(Carrier::EnhancedNumber),
            _ => None,
        },
        (
            ObjectKind::Bond,
            PropertyKind::UnknownXml(XmlName {
                namespace: None,
                local,
            }),
        ) => match local.as_str() {
            "Display" => Some(Carrier::Display),
            "Display2" => Some(Carrier::Display2),
            _ => None,
        },
        (ObjectKind::Bond, PropertyKind::UnknownBinary(0x0601)) => Some(Carrier::Display),
        _ => None,
    }
}

/// Whether the projection reads this property of a node or bond itself.
pub(super) fn is_carrier(owner: &ObjectKind, kind: &PropertyKind) -> bool {
    carrier(owner, kind).is_some()
}

/// Whether `object` has a property the projection neither reads as chemistry nor as one of the
/// stereo carriers: such an object is refused, because the property may affect chemistry.
pub(super) fn has_unread_property(object: &Object) -> bool {
    object.properties.iter().any(|property| {
        matches!(
            property.kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
        ) && carrier(&object.kind, &property.kind).is_none()
    })
}

fn invalid(path: &str, what: &str) -> InterchangeError {
    fail(
        "invalid_stereo_attribute",
        format!("{path} has {what}; nothing was imported."),
    )
}

fn note(
    path: String,
    feature: &str,
    disposition: LossDisposition,
    impact: SemanticImpact,
    reason: String,
) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path: path,
        disposition,
        reason,
        semantic_impact: impact,
    }
}

/// The first 32 characters of a source value, for a message.
fn shown(text: &str) -> String {
    text.chars().take(32).collect()
}

/// One enhanced-stereo group a node states: its kind and number (`0` for an omitted number and
/// for an absolute group, which has none).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Membership {
    pub(super) kind: GroupKind,
    pub(super) number: i32,
}

/// What a node's `EnhancedStereoType` byte or text names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnhancedType {
    Unspecified,
    None,
    Kind(GroupKind),
}

fn enhanced_type(value: &PropertyValue, path: &str) -> Result<EnhancedType, InterchangeError> {
    match value {
        PropertyValue::Text(text) => match text.as_str() {
            "Unspecified" => Ok(EnhancedType::Unspecified),
            "None" => Ok(EnhancedType::None),
            "Absolute" => Ok(EnhancedType::Kind(GroupKind::Absolute)),
            "Or" => Ok(EnhancedType::Kind(GroupKind::Or)),
            "And" => Ok(EnhancedType::Kind(GroupKind::And)),
            other => Err(invalid(
                path,
                &format!("an unknown EnhancedStereoType {:?}", shown(other)),
            )),
        },
        PropertyValue::Bytes(bytes) => match bytes.as_slice() {
            [0] => Ok(EnhancedType::Unspecified),
            [1] => Ok(EnhancedType::None),
            [2] => Ok(EnhancedType::Kind(GroupKind::Absolute)),
            [3] => Ok(EnhancedType::Kind(GroupKind::Or)),
            [4] => Ok(EnhancedType::Kind(GroupKind::And)),
            other => Err(invalid(
                path,
                &format!(
                    "an enhanced stereo type of {} bytes or an unknown value",
                    other.len()
                ),
            )),
        },
        _ => Err(invalid(
            path,
            "an enhanced stereo type of an unexpected kind",
        )),
    }
}

/// The group number as the file states it: any 32-bit integer in CDXML, and in CDX the two
/// bytes read as little-endian **unsigned** 16-bit (so 128 stays 128 and 258 stays 258; the
/// reader of another program that is observed to turn them into -128 and 2 is not copied).
fn enhanced_number(value: &PropertyValue, path: &str) -> Result<i32, InterchangeError> {
    match value {
        PropertyValue::Text(text) => text.trim().parse::<i32>().map_err(|_| {
            invalid(
                path,
                &format!(
                    "an EnhancedStereoGroupNum {:?} that is not a 32-bit integer",
                    shown(text)
                ),
            )
        }),
        PropertyValue::Bytes(bytes) => match bytes.as_slice() {
            [low, high] => Ok(i32::from(u16::from_le_bytes([*low, *high]))),
            other => Err(invalid(
                path,
                &format!(
                    "a group number of {} bytes where two are expected",
                    other.len()
                ),
            )),
        },
        _ => Err(invalid(path, "a group number of an unexpected kind")),
    }
}

/// The CDXML name of a binary `Atom_CIPStereochemistry` (`0437`, INT8) value, from the public
/// enumeration: 0 `U`, 1 `N`, 2 `R`, 3 `S`, 4 `r`, 5 `s`, 6 `u`. The width is checked first.
fn cip_name<'a>(bytes: &[u8], path: &str) -> Result<&'a str, InterchangeError> {
    const NAMES: [&str; 7] = ["U", "N", "R", "S", "r", "s", "u"];
    let [byte] = bytes else {
        return Err(invalid(
            path,
            &format!(
                "an atom CIP label of {} bytes where one is expected",
                bytes.len()
            ),
        ));
    };
    NAMES
        .get(usize::from(*byte))
        .copied()
        .ok_or_else(|| invalid(path, "an atom CIP label outside the public enumeration"))
}

fn check_geometry(value: &PropertyValue, path: &str) -> Result<(), InterchangeError> {
    let tetrahedral = match value {
        PropertyValue::Text(text) => text == "Tetrahedral",
        PropertyValue::Bytes(bytes) => match bytes.as_slice() {
            [byte] => *byte == CDX_GEOMETRY_TETRAHEDRAL,
            other => {
                return Err(invalid(
                    path,
                    &format!("a Geometry of {} bytes where one is expected", other.len()),
                ));
            }
        },
        _ => false,
    };
    if tetrahedral {
        Ok(())
    } else {
        Err(fail(
            "unsupported_geometry",
            format!(
                "{path} is a Geometry other than Tetrahedral, whose meaning is not established; nothing was imported."
            ),
        ))
    }
}

/// `BondOrdering` lists bond identifiers around the atom (CDXML: integers separated by
/// spaces; CDX: 32-bit little-endian entries), with a zero slot where no bond is named. Read
/// only to check it is well formed: the configuration is derived from the drawn marks.
fn check_ordering(value: &PropertyValue, path: &str) -> Result<(), InterchangeError> {
    let entries = match value {
        PropertyValue::Text(text) => {
            let mut count = 0;
            for token in text.split_ascii_whitespace() {
                token.parse::<u32>().map_err(|_| {
                    invalid(path, "a BondOrdering entry that is not an unsigned integer")
                })?;
                count += 1;
            }
            count
        }
        PropertyValue::Bytes(bytes) if bytes.len() % 4 == 0 => bytes.len() / 4,
        _ => {
            return Err(invalid(
                path,
                "a BondOrdering that is not whole 32-bit entries",
            ));
        }
    };
    if entries == 0 || entries > MAX_ORDERING_ENTRIES {
        return Err(invalid(path, "a BondOrdering with no entries or too many"));
    }
    Ok(())
}

/// Reads a node's stereo carriers. Pushes a no-consent note for each statement that is not
/// stored (never a configuration: that is derived from the marks), and returns the group the
/// atom states, if any. `path` is the node's source path.
pub(super) fn read_node(
    object: &Object,
    path: &str,
    losses: &mut Vec<Loss>,
) -> Result<Option<Membership>, InterchangeError> {
    let mut seen: Vec<Carrier> = Vec::new();
    let mut enhanced = EnhancedType::Unspecified;
    let mut number: Option<(i32, String)> = None;
    for (index, property) in object.properties.iter().enumerate() {
        let Some(found) = carrier(&object.kind, &property.kind) else {
            continue;
        };
        let at = format!("{path}.properties[{index}]");
        if seen.contains(&found) {
            return Err(invalid(&at, "a carrier stated twice"));
        }
        seen.push(found);
        match found {
            Carrier::Geometry => {
                check_geometry(&property.value, &at)?;
                losses.push(note(
                    at,
                    "stereo_geometry",
                    LossDisposition::Dropped,
                    SemanticImpact::Stereo,
                    "Geometry=Tetrahedral is a hint of the source program and states no configuration; the configuration is derived from the drawn marks and positions.".into(),
                ));
            }
            Carrier::BondOrdering => {
                check_ordering(&property.value, &at)?;
                losses.push(note(
                    at,
                    "bond_ordering",
                    LossDisposition::Dropped,
                    SemanticImpact::Stereo,
                    "The neighbour order of the source program is not stored; the configuration is derived from the drawn marks and positions.".into(),
                ));
            }
            Carrier::Label => {
                let label = match &property.value {
                    PropertyValue::Text(label) => label.as_str(),
                    PropertyValue::Bytes(bytes) => cip_name(bytes, &at)?,
                    _ => return Err(invalid(path, "an AS label of an unexpected kind")),
                };
                losses.push(note(
                    at,
                    "cip_label_not_stored",
                    LossDisposition::Dropped,
                    SemanticImpact::Stereo,
                    format!(
                        "The label {:?} was computed by the source program; it is not interpreted and never stored as a configuration.",
                        shown(label)
                    ),
                ));
            }
            Carrier::EnhancedType => enhanced = enhanced_type(&property.value, path)?,
            Carrier::EnhancedNumber => {
                number = Some((enhanced_number(&property.value, path)?, at));
            }
            Carrier::Display | Carrier::Display2 => {}
        }
    }
    let type_at = |losses: &mut Vec<Loss>, feature: &str, disposition, reason: String| {
        losses.push(note(
            path.to_owned(),
            feature,
            disposition,
            SemanticImpact::Stereo,
            reason,
        ));
    };
    match (enhanced, number) {
        (EnhancedType::Kind(GroupKind::Absolute), stated) => {
            if let Some((stated, at)) = stated.filter(|(value, _)| *value != 0) {
                losses.push(note(
                    at,
                    "stereo_group_number",
                    LossDisposition::Dropped,
                    SemanticImpact::Stereo,
                    format!("An absolute group has no number; the stated {stated} is not stored."),
                ));
            }
            Ok(Some(Membership {
                kind: GroupKind::Absolute,
                number: 0,
            }))
        }
        (EnhancedType::Kind(kind), stated) => {
            if stated.is_none() {
                type_at(
                    losses,
                    "stereo_group_number",
                    LossDisposition::Normalized,
                    format!(
                        "The atom states a {} group without a number; an omitted number is read as 0.",
                        kind.as_str()
                    ),
                );
            }
            Ok(Some(Membership {
                kind,
                number: stated.map_or(0, |(value, _)| value),
            }))
        }
        (EnhancedType::None, stated) => {
            type_at(
                losses,
                "enhanced_stereo_none",
                LossDisposition::Dropped,
                "The atom states that it is in no enhanced stereo group; it joins none.".into(),
            );
            note_unused_number(stated, losses);
            Ok(None)
        }
        (EnhancedType::Unspecified, stated) => {
            note_unused_number(stated, losses);
            Ok(None)
        }
    }
}

fn note_unused_number(stated: Option<(i32, String)>, losses: &mut Vec<Loss>) {
    if let Some((value, at)) = stated.filter(|(value, _)| *value != 0) {
        losses.push(note(
            at,
            "stereo_group_number",
            LossDisposition::Dropped,
            SemanticImpact::Stereo,
            format!("The group number {value} names no group because the atom states no group type; it is not stored."),
        ));
    }
}

/// A bond display value of the public `kCDXProp_Bond_Display` enumeration (IUPAC FAIRSpec CDX
/// SDK page `properties/Bond_Display.htm`), in the page's numbering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Look {
    Solid,
    Dash,
    Hash,
    HashWedgeBegin,
    HashWedgeEnd,
    Bold,
    WedgeBegin,
    WedgeEnd,
    Wavy,
    HollowBegin,
    HollowEnd,
    WavyWedgeBegin,
    WavyWedgeEnd,
    Dot,
    DashDot,
}

/// The one table of display values: the position is the CDX `INT16` value, the text is the
/// CDXML spelling. Both formats read through it, so they classify identically.
const LOOKS: [(Look, &str); 15] = [
    (Look::Solid, "Solid"),
    (Look::Dash, "Dash"),
    (Look::Hash, "Hash"),
    (Look::HashWedgeBegin, "WedgedHashBegin"),
    (Look::HashWedgeEnd, "WedgedHashEnd"),
    (Look::Bold, "Bold"),
    (Look::WedgeBegin, "WedgeBegin"),
    (Look::WedgeEnd, "WedgeEnd"),
    (Look::Wavy, "Wavy"),
    (Look::HollowBegin, "HollowWedgeBegin"),
    (Look::HollowEnd, "HollowWedgeEnd"),
    (Look::WavyWedgeBegin, "WavyWedgeBegin"),
    (Look::WavyWedgeEnd, "WavyWedgeEnd"),
    (Look::Dot, "Dot"),
    (Look::DashDot, "DashDot"),
];

/// The value a display property states, as the table's entry, or `None` for a value outside it.
fn read_look(
    value: &PropertyValue,
    at: &str,
) -> Result<Option<(Look, &'static str)>, InterchangeError> {
    match value {
        PropertyValue::Text(text) => Ok(LOOKS.iter().find(|(_, name)| name == text).copied()),
        PropertyValue::Bytes(bytes) => {
            let Ok(pair) = <[u8; 2]>::try_from(bytes.as_slice()) else {
                return Err(invalid(at, "a display value that is not two bytes"));
            };
            let number = i16::from_le_bytes(pair);
            Ok(usize::try_from(number)
                .ok()
                .and_then(|index| LOOKS.get(index))
                .copied())
        }
        _ => Err(invalid(at, "a display value of an unexpected kind")),
    }
}

/// The mark a display value states on a bond of `order` (the source's order bits): the narrow
/// end is the begin atom (`B`) for `...Begin` and the end atom (`E`) for `...End`, on a single
/// bond; `Wavy` on a double bond is the double-either (crossed) bond, as the public page says.
fn mark_of(look: Look, order: i64) -> Option<StereoMark> {
    let narrow = |end| (order == 1).then_some(end);
    match look {
        Look::WedgeBegin => narrow(BondEnd::Begin).map(|narrow| StereoMark::Wedge { narrow }),
        Look::WedgeEnd => narrow(BondEnd::End).map(|narrow| StereoMark::Wedge { narrow }),
        Look::HashWedgeBegin => narrow(BondEnd::Begin).map(|narrow| StereoMark::Hash { narrow }),
        Look::HashWedgeEnd => narrow(BondEnd::End).map(|narrow| StereoMark::Hash { narrow }),
        Look::Wavy => match order {
            1 => Some(StereoMark::Wavy),
            2 => Some(StereoMark::Crossed),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the value is a drawing style the public enumeration gives no stereo meaning
/// (`Dash`, `Bold`, `Dot`, `DashDot`).
fn is_style(look: Look) -> bool {
    matches!(look, Look::Dash | Look::Bold | Look::Dot | Look::DashDot)
}

/// Reads a bond's display values for a bond of `order`. Returns the mark the bond holds. A
/// shape that may state stereo but cannot be held (a uniform hash, a hollow or wavy wedge, a
/// wedge or hash on a bond that is not single, any mark in `Display2`) imports a plain bond and
/// reports the consent-required loss `stereo_mark_unplaceable`; a drawing style is a no-consent
/// note; a value outside the enumeration refuses the bond.
pub(super) fn read_bond(
    object: &Object,
    path: &str,
    order: i64,
    losses: &mut Vec<Loss>,
) -> Result<Option<StereoMark>, InterchangeError> {
    let mut mark = None;
    let mut seen: Vec<Carrier> = Vec::new();
    for (index, property) in object.properties.iter().enumerate() {
        let Some(found) = carrier(&object.kind, &property.kind) else {
            continue;
        };
        let at = format!("{path}.properties[{index}]");
        if seen.contains(&found) {
            return Err(invalid(&at, "a display value stated twice"));
        }
        seen.push(found);
        let Some((value, name)) = read_look(&property.value, &at)? else {
            let stated = match &property.value {
                PropertyValue::Text(text) => format!("{:?}", shown(text)),
                PropertyValue::Bytes(bytes) => format!("{bytes:?}"),
                _ => String::new(),
            };
            return Err(fail(
                "unsupported_bond",
                format!(
                    "{path} has the display value {stated}, which is not in the public enumeration; nothing was imported."
                ),
            ));
        };
        let second_line = found == Carrier::Display2;
        if value == Look::Solid {
            continue;
        }
        if is_style(value) {
            losses.push(note(
                at,
                "bond_display",
                LossDisposition::Dropped,
                SemanticImpact::Rendering,
                format!("The bond style {name} is a drawing style; the bond is imported as a plain bond of its order."),
            ));
            continue;
        }
        match mark_of(value, order).filter(|_| !second_line) {
            Some(held) => mark = Some(held),
            None => {
                let why = if second_line {
                    "is a second-line display, which a bond cannot hold"
                } else if mark_of(value, 1).is_none() && mark_of(value, 2).is_none() {
                    "has no equivalent mark in the editor"
                } else {
                    "is on a bond whose order cannot carry it"
                };
                losses.push(Loss {
                    consent: LossConsent::Required,
                    feature: "stereo_mark_unplaceable".into(),
                    source_path: at,
                    disposition: LossDisposition::Dropped,
                    reason: format!("The display value {name} may state stereochemistry and {why}; the bond is imported as a plain bond."),
                    semantic_impact: SemanticImpact::Stereo,
                });
            }
        }
    }
    Ok(mark)
}

/// The diagnostic family of each closed reason this reader adds, carried as `details.family`
/// beside the fine reason (the `code`), as the CT-file reasons map to their families. `None`
/// for a code this module does not introduce (the older `unsupported_bond` and
/// `unsupported_node` keep their wire form).
#[must_use]
pub(crate) fn reason_family(reason: &str) -> Option<&'static str> {
    match reason {
        "invalid_stereo_attribute" | "stereo_group_member_not_stereocenter" => {
            Some("malformed_input")
        }
        "stereo_group_drawing_unreadable" | "unsupported_geometry" => Some("unsupported_feature"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::interchange::Property;

    fn xml(name: &str) -> PropertyKind {
        PropertyKind::UnknownXml(XmlName {
            namespace: None,
            local: name.into(),
        })
    }

    fn text(name: &str, value: &str) -> Property {
        Property {
            kind: xml(name),
            value: PropertyValue::Text(value.into()),
        }
    }

    fn binary(tag: u16, bytes: &[u8]) -> Property {
        Property {
            kind: PropertyKind::UnknownBinary(tag),
            value: PropertyValue::Bytes(bytes.to_vec()),
        }
    }

    fn node(properties: Vec<Property>) -> Object {
        Object {
            kind: ObjectKind::Node,
            id: Some(1),
            properties,
            children: Vec::new(),
        }
    }

    fn bond(properties: Vec<Property>) -> Object {
        Object {
            kind: ObjectKind::Bond,
            id: Some(2),
            properties,
            children: Vec::new(),
        }
    }

    fn member(properties: Vec<Property>) -> Result<Option<Membership>, InterchangeError> {
        read_node(&node(properties), "n", &mut Vec::new())
    }

    fn group(kind: GroupKind, number: i32) -> Result<Option<Membership>, InterchangeError> {
        Ok(Some(Membership { kind, number }))
    }

    #[test]
    fn a_cdx_group_number_is_two_unsigned_little_endian_bytes() {
        // 128 and 258 are the values another reader turns into -128 and 2; here they stay.
        for (bytes, expected) in [
            ([0x00, 0x00], 0),
            ([0x7f, 0x00], 127),
            ([0x80, 0x00], 128),
            ([0x02, 0x01], 258),
            ([0xff, 0xff], 65535),
        ] {
            assert_eq!(
                member(vec![binary(0x0446, &[3]), binary(0x0447, &bytes)]),
                group(GroupKind::Or, expected),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn a_cdx_group_number_of_another_width_is_refused() {
        for bytes in [&[][..], &[1], &[1, 0, 0]] {
            let error = member(vec![binary(0x0446, &[3]), binary(0x0447, bytes)]).unwrap_err();
            assert_eq!(error.code, "invalid_stereo_attribute", "{bytes:?}");
        }
    }

    #[test]
    fn a_cdxml_group_number_is_any_32_bit_integer() {
        for (value, expected) in [
            ("0", 0),
            ("1", 1),
            ("128", 128),
            ("258", 258),
            ("-128", -128),
            ("2147483647", i32::MAX),
            ("-2147483648", i32::MIN),
        ] {
            assert_eq!(
                member(vec![
                    text("EnhancedStereoType", "And"),
                    text("EnhancedStereoGroupNum", value)
                ]),
                group(GroupKind::And, expected),
                "{value}"
            );
        }
        for value in ["2147483648", "1.5", "", "one", "0x10"] {
            let error = member(vec![
                text("EnhancedStereoType", "And"),
                text("EnhancedStereoGroupNum", value),
            ])
            .unwrap_err();
            assert_eq!(error.code, "invalid_stereo_attribute", "{value:?}");
        }
    }

    #[test]
    fn the_enhanced_type_names_and_bytes_agree_and_nothing_else_is_read() {
        for (name, byte, expected) in [
            ("Absolute", 2, Some(GroupKind::Absolute)),
            ("Or", 3, Some(GroupKind::Or)),
            ("And", 4, Some(GroupKind::And)),
            ("None", 1, None),
            ("Unspecified", 0, None),
        ] {
            let by_name = member(vec![text("EnhancedStereoType", name)]).unwrap();
            let by_byte = member(vec![binary(0x0446, &[byte])]).unwrap();
            let kind = |found: Option<Membership>| found.map(|m| m.kind);
            assert_eq!(kind(by_name), expected, "{name}");
            assert_eq!(kind(by_byte), expected, "byte {byte}");
        }
        for property in [
            text("EnhancedStereoType", "or"),
            text("EnhancedStereoType", "Both"),
            binary(0x0446, &[5]),
            binary(0x0446, &[3, 0]),
        ] {
            assert_eq!(
                member(vec![property]).unwrap_err().code,
                "invalid_stereo_attribute"
            );
        }
    }

    #[test]
    fn an_omitted_number_is_zero_and_says_so() {
        let mut losses = Vec::new();
        let found = read_node(
            &node(vec![text("EnhancedStereoType", "Or")]),
            "n",
            &mut losses,
        );
        assert_eq!(found, group(GroupKind::Or, 0));
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "stereo_group_number");
        assert_eq!(losses[0].disposition, LossDisposition::Normalized);
        assert_eq!(losses[0].consent, LossConsent::NotRequired);
        // A stated 0 is the same group and needs no note.
        let mut losses = Vec::new();
        let stated = read_node(
            &node(vec![
                text("EnhancedStereoType", "Or"),
                text("EnhancedStereoGroupNum", "0"),
            ]),
            "n",
            &mut losses,
        );
        assert_eq!(stated, group(GroupKind::Or, 0));
        assert!(losses.is_empty(), "{losses:?}");
    }

    #[test]
    fn an_absolute_atom_has_no_number_and_a_stated_one_is_reported() {
        let mut losses = Vec::new();
        let found = read_node(
            &node(vec![
                text("EnhancedStereoType", "Absolute"),
                text("EnhancedStereoGroupNum", "5"),
            ]),
            "n",
            &mut losses,
        );
        assert_eq!(found, group(GroupKind::Absolute, 0));
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "stereo_group_number");
        assert_eq!(losses[0].disposition, LossDisposition::Dropped);
        assert_eq!(losses[0].source_path, "n.properties[1]");
        let mut quiet = Vec::new();
        read_node(
            &node(vec![
                text("EnhancedStereoType", "Absolute"),
                text("EnhancedStereoGroupNum", "0"),
            ]),
            "n",
            &mut quiet,
        )
        .unwrap();
        assert!(quiet.is_empty(), "{quiet:?}");
    }

    #[test]
    fn none_is_noted_and_unspecified_is_silent_and_neither_joins_a_group() {
        let mut losses = Vec::new();
        assert_eq!(
            read_node(
                &node(vec![text("EnhancedStereoType", "None")]),
                "n",
                &mut losses
            ),
            Ok(None)
        );
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "enhanced_stereo_none");
        let mut silent = Vec::new();
        assert_eq!(
            read_node(
                &node(vec![text("EnhancedStereoType", "Unspecified")]),
                "n",
                &mut silent
            ),
            Ok(None)
        );
        assert!(silent.is_empty(), "{silent:?}");
        // A number with no type names no group.
        let mut orphan = Vec::new();
        assert_eq!(
            read_node(
                &node(vec![text("EnhancedStereoGroupNum", "3")]),
                "n",
                &mut orphan
            ),
            Ok(None)
        );
        assert_eq!(orphan[0].feature, "stereo_group_number");
    }

    #[test]
    fn hints_are_noted_not_stored_and_a_geometry_other_than_tetrahedral_is_refused() {
        let mut losses = Vec::new();
        read_node(
            &node(vec![
                text("Geometry", "Tetrahedral"),
                text("BondOrdering", "8 9 0 10"),
                text("AS", "S"),
            ]),
            "n",
            &mut losses,
        )
        .unwrap();
        let features: Vec<&str> = losses.iter().map(|l| l.feature.as_str()).collect();
        assert_eq!(
            features,
            ["stereo_geometry", "bond_ordering", "cip_label_not_stored"]
        );
        assert!(losses.iter().all(|l| l.consent == LossConsent::NotRequired));
        // The binary spellings.
        let mut binary_losses = Vec::new();
        let ordering: Vec<u8> = [8_u32, 9, 0, 10]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        read_node(
            &node(vec![binary(0x0430, &[7]), binary(0x0431, &ordering)]),
            "n",
            &mut binary_losses,
        )
        .unwrap();
        assert_eq!(binary_losses.len(), 2);
        for refused in [
            text("Geometry", "Planar"),
            text("Geometry", "tetrahedral"),
            binary(0x0430, &[6]),
        ] {
            assert_eq!(
                member(vec![refused]).unwrap_err().code,
                "unsupported_geometry"
            );
        }
        // A width other than one byte is a malformed payload, whatever the bytes say.
        for malformed in [&[][..], &[7, 0][..], &[6, 7][..], &[7, 7, 7][..]] {
            assert_eq!(
                member(vec![binary(0x0430, malformed)]).unwrap_err().code,
                "invalid_stereo_attribute",
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn a_binary_cip_label_is_a_hint_width_first_then_the_enumeration() {
        for (value, name) in [
            (0_u8, "U"),
            (1, "N"),
            (2, "R"),
            (3, "S"),
            (4, "r"),
            (5, "s"),
            (6, "u"),
        ] {
            let mut losses = Vec::new();
            let membership =
                read_node(&node(vec![binary(0x0437, &[value])]), "n", &mut losses).unwrap();
            assert!(membership.is_none());
            assert_eq!(losses.len(), 1, "{losses:?}");
            assert_eq!(losses[0].feature, "cip_label_not_stored");
            assert!(losses[0].reason.contains(name), "{}", losses[0].reason);
        }
        for bad in [&[][..], &[0, 0][..], &[7][..], &[255][..]] {
            assert_eq!(
                member(vec![binary(0x0437, bad)]).unwrap_err().code,
                "invalid_stereo_attribute",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_published_bond_ordering_example_is_read() {
        // The public page's own example: four entries, 1 2 3 4, sixteen bytes, no count.
        let example = [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0];
        let mut losses = Vec::new();
        read_node(&node(vec![binary(0x0431, &example)]), "n", &mut losses).unwrap();
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "bond_ordering");
        for entries in [1_usize, 3] {
            let bytes: Vec<u8> = (0..entries).flat_map(|i| [i as u8 + 1, 0, 0, 0]).collect();
            read_node(&node(vec![binary(0x0431, &bytes)]), "n", &mut Vec::new()).unwrap();
        }
    }

    #[test]
    fn a_malformed_bond_ordering_is_refused() {
        let too_many = "1 ".repeat(MAX_ORDERING_ENTRIES + 1);
        for value in ["", "a b", "1 -2", too_many.as_str()] {
            assert_eq!(
                member(vec![text("BondOrdering", value)]).unwrap_err().code,
                "invalid_stereo_attribute",
                "{value:?}"
            );
        }
        assert_eq!(
            member(vec![binary(0x0431, &[1, 0, 0])]).unwrap_err().code,
            "invalid_stereo_attribute"
        );
        // The published CDXObjectIDArray has no count: the length is implicit and each entry is
        // a 32-bit little-endian value, so a 16-bit count in front makes the length 2 + 4n
        // and is not a layout that is read.
        let mut prefixed = vec![4, 0];
        prefixed.extend_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0]);
        assert_eq!(
            member(vec![binary(0x0431, &prefixed)]).unwrap_err().code,
            "invalid_stereo_attribute"
        );
        assert_eq!(
            member(vec![binary(0x0431, &[])]).unwrap_err().code,
            "invalid_stereo_attribute"
        );
    }

    #[test]
    fn a_carrier_stated_twice_is_refused() {
        let error =
            member(vec![text("Geometry", "Tetrahedral"), binary(0x0430, &[7])]).unwrap_err();
        assert_eq!(error.code, "invalid_stereo_attribute");
    }

    fn display(
        order: i64,
        properties: Vec<Property>,
    ) -> (Result<Option<StereoMark>, InterchangeError>, Vec<Loss>) {
        let mut losses = Vec::new();
        let read = read_bond(&bond(properties), "b", order, &mut losses);
        (read, losses)
    }

    #[test]
    fn begin_and_end_name_the_narrow_end() {
        for (value, mark) in [
            (
                "WedgeBegin",
                StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                },
            ),
            (
                "WedgeEnd",
                StereoMark::Wedge {
                    narrow: BondEnd::End,
                },
            ),
            (
                "WedgedHashBegin",
                StereoMark::Hash {
                    narrow: BondEnd::Begin,
                },
            ),
            (
                "WedgedHashEnd",
                StereoMark::Hash {
                    narrow: BondEnd::End,
                },
            ),
            ("Wavy", StereoMark::Wavy),
        ] {
            let (read, losses) = display(1, vec![text("Display", value)]);
            assert_eq!(read, Ok(Some(mark)), "{value}");
            assert!(losses.is_empty(), "{value}: {losses:?}");
        }
    }

    #[test]
    fn plain_styles_import_plain_and_only_the_decorated_ones_are_noted() {
        let (read, losses) = display(1, vec![text("Display", "Solid")]);
        assert_eq!((read, losses.len()), (Ok(None), 0));
        for value in ["Bold", "Dash", "Dot", "DashDot"] {
            let (read, losses) = display(2, vec![text("Display", value)]);
            assert_eq!(read, Ok(None), "{value}");
            assert_eq!(losses.len(), 1, "{value}");
            assert_eq!(losses[0].feature, "bond_display");
            assert_eq!(losses[0].consent, LossConsent::NotRequired);
            assert_eq!(losses[0].semantic_impact, SemanticImpact::Rendering);
        }
    }

    #[test]
    fn shapes_a_bond_cannot_hold_import_plain_with_a_consent_required_loss() {
        let cases = [
            (1, "Display", "Hash"),
            (1, "Display", "HollowWedgeBegin"),
            (1, "Display", "HollowWedgeEnd"),
            (1, "Display", "WavyWedgeBegin"),
            (1, "Display", "WavyWedgeEnd"),
            (2, "Display", "WedgeBegin"),
            (4, "Display", "WedgedHashEnd"),
            (128, "Display", "WedgeEnd"),
            (4, "Display", "Wavy"),
            (1, "Display2", "WedgeBegin"),
            (1, "Display2", "Wavy"),
        ];
        for (order, name, value) in cases {
            let (read, losses) = display(order, vec![text(name, value)]);
            assert_eq!(read, Ok(None), "{name}={value} on order {order}");
            assert_eq!(losses.len(), 1, "{name}={value}: {losses:?}");
            assert_eq!(losses[0].feature, "stereo_mark_unplaceable");
            assert_eq!(losses[0].consent, LossConsent::Required);
            assert_eq!(losses[0].semantic_impact, SemanticImpact::Stereo);
            assert_eq!(losses[0].source_path, "b.properties[0]");
        }
    }

    #[test]
    fn wavy_on_a_double_bond_is_the_crossed_mark() {
        // Public page: "Wavy is treated as a double-either (crossed) bond".
        let (read, losses) = display(2, vec![text("Display", "Wavy")]);
        assert_eq!(read, Ok(Some(StereoMark::Crossed)));
        assert!(losses.is_empty(), "{losses:?}");
        // On a triple bond a wavy line has no mark to become: plain, with the consent loss.
        let (read, losses) = display(3, vec![text("Display", "Wavy")]);
        assert_eq!(read, Ok(None));
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "stereo_mark_unplaceable");
    }

    fn wire(value: i16) -> Property {
        binary(0x0601, &value.to_le_bytes())
    }

    #[test]
    fn the_binary_display_numbers_read_exactly_as_their_cdxml_names() {
        // The page's numbering; one table serves both spellings.
        let names = [
            "Solid",
            "Dash",
            "Hash",
            "WedgedHashBegin",
            "WedgedHashEnd",
            "Bold",
            "WedgeBegin",
            "WedgeEnd",
            "Wavy",
            "HollowWedgeBegin",
            "HollowWedgeEnd",
            "WavyWedgeBegin",
            "WavyWedgeEnd",
            "Dot",
            "DashDot",
        ];
        for (number, name) in names.iter().enumerate() {
            for order in [1, 2, 4] {
                let by_name = display(order, vec![text("Display", name)]);
                let by_number = display(order, vec![wire(number as i16)]);
                assert_eq!(by_name.0, by_number.0, "{name} on order {order}");
                let features = |l: &[Loss]| -> Vec<(String, String)> {
                    l.iter()
                        .map(|l| (l.feature.clone(), l.reason.clone()))
                        .collect()
                };
                assert_eq!(features(&by_name.1), features(&by_number.1), "{name}");
            }
        }
        // Spot checks against the page, not against the table under test.
        assert_eq!(
            display(1, vec![wire(6)]).0,
            Ok(Some(StereoMark::Wedge {
                narrow: BondEnd::Begin
            }))
        );
        assert_eq!(
            display(1, vec![wire(4)]).0,
            Ok(Some(StereoMark::Hash {
                narrow: BondEnd::End
            }))
        );
        assert_eq!(display(1, vec![wire(8)]).0, Ok(Some(StereoMark::Wavy)));
        assert_eq!(display(1, vec![wire(0)]).0, Ok(None));
    }

    #[test]
    fn a_binary_display_outside_the_enumeration_or_of_another_width_is_refused() {
        for number in [15_i16, 100, -1, i16::MIN, i16::MAX] {
            let (read, _) = display(1, vec![wire(number)]);
            assert_eq!(read.unwrap_err().code, "unsupported_bond", "{number}");
        }
        for bytes in [&[][..], &[6], &[6, 0, 0]] {
            let (read, _) = display(1, vec![binary(0x0601, bytes)]);
            assert_eq!(
                read.unwrap_err().code,
                "invalid_stereo_attribute",
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn a_second_line_style_is_noted_and_does_not_replace_the_mark() {
        let (read, losses) = display(
            1,
            vec![text("Display", "WedgeEnd"), text("Display2", "Bold")],
        );
        assert_eq!(
            read,
            Ok(Some(StereoMark::Wedge {
                narrow: BondEnd::End
            }))
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].feature, "bond_display");
    }

    #[test]
    fn a_display_value_the_public_list_lacks_is_refused() {
        for value in ["wedgebegin", "", "Solid ", "Future"] {
            let (read, _) = display(1, vec![text("Display", value)]);
            assert_eq!(read.unwrap_err().code, "unsupported_bond", "{value:?}");
        }
        let (twice, _) = display(1, vec![text("Display", "Solid"), text("Display", "Bold")]);
        assert_eq!(twice.unwrap_err().code, "invalid_stereo_attribute");
    }

    #[test]
    fn only_the_confirmed_properties_are_carriers() {
        assert!(is_carrier(&ObjectKind::Bond, &xml("Display")));
        assert!(!is_carrier(&ObjectKind::Bond, &xml("DoublePosition")));
        assert!(!is_carrier(&ObjectKind::Bond, &xml("CIP")));
        assert!(is_carrier(
            &ObjectKind::Bond,
            &PropertyKind::UnknownBinary(0x0601)
        ));
        for tag in [0x0602, 0x0603] {
            assert!(!is_carrier(
                &ObjectKind::Bond,
                &PropertyKind::UnknownBinary(tag)
            ));
        }
        assert!(!is_carrier(&ObjectKind::Node, &xml("Display")));
        assert!(!is_carrier(
            &ObjectKind::Node,
            &xml("ShowAtomEnhancedStereo")
        ));
        for tag in [0x0430, 0x0431, 0x0437, 0x0446, 0x0447] {
            assert!(is_carrier(
                &ObjectKind::Node,
                &PropertyKind::UnknownBinary(tag)
            ));
        }
        for tag in [0x0445, 0x0448] {
            assert!(!is_carrier(
                &ObjectKind::Node,
                &PropertyKind::UnknownBinary(tag)
            ));
        }
        let namespaced = PropertyKind::UnknownXml(XmlName {
            namespace: Some("urn:x".into()),
            local: "Geometry".into(),
        });
        assert!(!is_carrier(&ObjectKind::Node, &namespaced));
    }

    #[test]
    fn every_closed_reason_has_a_family() {
        for reason in [
            "invalid_stereo_attribute",
            "stereo_group_member_not_stereocenter",
            "stereo_group_drawing_unreadable",
            "unsupported_geometry",
        ] {
            assert!(reason_family(reason).is_some(), "{reason}");
        }
        assert_eq!(reason_family("something_else"), None);
    }
}
