//! Closed native graphics/group clipboard data. No document commands are decoded from text.

use chemifree_domain::document::{ObjectId, Point};
use chemifree_domain::scene::{
    Graphic, GraphicShape, ReferenceTarget, SceneObject, SceneReference,
};
use chemifree_domain::scene_clipboard::{Fragment, FragmentObject, SceneFragmentError};
use chemifree_domain::settings::Settings;
use serde::{Deserialize, Serialize};

use crate::ApplicationError;
use crate::clipboard::{self, MAX_CLIPBOARD_BYTES};
use crate::contract::{
    self, GraphicShapeDto, PointDto, SelectionDto, SettingsDto, SettingsEditDto,
};

pub(crate) struct Paste<'a> {
    pub expected_revision: u64,
    pub payload: &'a str,
    pub offset: PointDto,
    pub selection_before: Option<SelectionDto>,
    pub fragment: &'a Fragment,
}

// A future body can differ completely. Inspect only its common typed header before
// imposing the closed graphics schema. In particular, graph version 1 has no kind field.
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u64,
}

#[derive(Deserialize)]
struct KindHeader {
    kind: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: String,
    version: u32,
    kind: String,
    settings: SettingsDto,
    roots: Vec<String>,
    objects: Vec<ObjectData>,
    references: Vec<ReferenceData>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ObjectData {
    Graphic {
        id: String,
        shape: GraphicShapeDto,
        points: [PointData; 2],
    },
    Group {
        id: String,
        children: Vec<String>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PointData {
    x: f64,
    y: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReferenceData {
    source: String,
    target: TargetData,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TargetData {
    Object { id: String },
}

pub(crate) fn domain_refusal(error: SceneFragmentError) -> ApplicationError {
    clipboard::refusal(error.as_str(), error.to_string())
}

fn invalid(message: impl Into<String>) -> ApplicationError {
    clipboard::refusal("clipboard_fragment_invalid", message)
}

fn unsupported() -> ApplicationError {
    clipboard::refusal(
        "clipboard_format_unsupported",
        "Expected chemifree.clipboard version 2 with kind graphics",
    )
}

fn object_id(value: &str) -> Result<ObjectId, ApplicationError> {
    ObjectId::parse(value).ok_or_else(|| {
        invalid(format!(
            "Invalid clipboard-local object identifier {value:?}"
        ))
    })
}

fn settings(value: SettingsDto) -> Result<Settings, ApplicationError> {
    Settings::default()
        .edited(&contract::to_settings_edit(&SettingsEditDto {
            bond_length_pt: Some(value.bond_length_pt),
            line_width_pt: Some(value.line_width_pt),
            label_font: Some(value.label_font),
            label_weight: Some(value.label_weight),
            label_style: Some(value.label_style),
            label_underline: Some(value.label_underline),
            label_outline: Some(value.label_outline),
            label_shadow: Some(value.label_shadow),
            label_size_pt: Some(value.label_size_pt),
            carbon_labels: Some(value.carbon_labels),
            display_unit: Some(value.display_unit),
        }))
        .map_err(|error| invalid(error.to_string()))
}

fn byte_limit() -> ApplicationError {
    clipboard::refusal(
        "clipboard_byte_limit",
        "Native clipboard text exceeds 65536 UTF-8 bytes",
    )
}

/// Encodes a valid graphics-only forest, including its order and internal object references.
///
/// # Errors
/// Refuses an invalid domain fragment or text exceeding the native clipboard byte limit.
pub(crate) fn encode(fragment: &Fragment) -> Result<String, ApplicationError> {
    fragment.validate().map_err(domain_refusal)?;
    let mut objects = Vec::with_capacity(fragment.objects.len());
    for entry in &fragment.objects {
        objects.push(match &entry.object {
            SceneObject::Graphic(graphic) => ObjectData::Graphic {
                id: entry.id.to_string(),
                shape: match graphic.shape {
                    GraphicShape::Line => GraphicShapeDto::Line,
                    GraphicShape::Arrow => GraphicShapeDto::Arrow,
                    GraphicShape::Rectangle => GraphicShapeDto::Rectangle,
                },
                points: graphic.points.map(|point| PointData {
                    x: point.x(),
                    y: point.y(),
                }),
            },
            SceneObject::Group { children } => ObjectData::Group {
                id: entry.id.to_string(),
                children: children.iter().map(ToString::to_string).collect(),
            },
            SceneObject::Caption(_) => {
                return Err(invalid(
                    "Captions are outside the graphics clipboard profile",
                ));
            }
            SceneObject::Instance => {
                return Err(invalid(
                    "Molecule placements are outside the graphics clipboard profile",
                ));
            }
        });
    }
    let references = fragment
        .references
        .iter()
        .map(|reference| {
            let ReferenceTarget::Object(target) = reference.target else {
                return Err(invalid(
                    "A graphics clipboard reference must target an object",
                ));
            };
            Ok(ReferenceData {
                source: reference.source.to_string(),
                target: TargetData::Object {
                    id: target.to_string(),
                },
            })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    let payload = Payload {
        format: "chemifree.clipboard".into(),
        version: 2,
        kind: "graphics".into(),
        settings: contract::settings_view(&fragment.settings),
        roots: fragment.roots.iter().map(ToString::to_string).collect(),
        objects,
        references,
    };
    let text = serde_json::to_string(&payload).map_err(|error| invalid(error.to_string()))?;
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(byte_limit());
    }
    Ok(text)
}

/// Reads only the closed native graphics schema, then checks the entire untrusted forest.
///
/// # Errors
/// Refuses malformed JSON/headers, unsupported profiles, unknown/missing fields, invalid
/// identifiers, graph/instance objects, invalid domain content or excess UTF-8 bytes.
pub(crate) fn decode(text: &str) -> Result<Fragment, ApplicationError> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(byte_limit());
    }
    let value =
        crate::json::parse_strict(text.as_bytes()).map_err(|error| invalid(error.to_string()))?;
    let header = Header::deserialize(&value).map_err(|error| invalid(error.to_string()))?;
    if header.format != "chemifree.clipboard" || header.version != 2 {
        return Err(unsupported());
    }
    let kind = KindHeader::deserialize(&value).map_err(|error| invalid(error.to_string()))?;
    if kind.kind != "graphics" {
        return Err(unsupported());
    }
    let payload: Payload =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    let mut objects = Vec::with_capacity(payload.objects.len());
    for entry in payload.objects {
        objects.push(match entry {
            ObjectData::Graphic { id, shape, points } => {
                let [first, second] = points;
                FragmentObject {
                    id: object_id(&id)?,
                    object: SceneObject::Graphic(Graphic {
                        line_width_pt: None,
                        shape: match shape {
                            GraphicShapeDto::Line => GraphicShape::Line,
                            GraphicShapeDto::Arrow => GraphicShape::Arrow,
                            GraphicShapeDto::Rectangle => GraphicShape::Rectangle,
                        },
                        points: [
                            Point::new(first.x, first.y)
                                .map_err(|error| invalid(error.to_string()))?,
                            Point::new(second.x, second.y)
                                .map_err(|error| invalid(error.to_string()))?,
                        ],
                    }),
                }
            }
            ObjectData::Group { id, children } => FragmentObject {
                id: object_id(&id)?,
                object: SceneObject::Group {
                    children: children
                        .iter()
                        .map(|id| object_id(id))
                        .collect::<Result<Vec<_>, _>>()?,
                },
            },
        });
    }
    let references = payload
        .references
        .into_iter()
        .map(|reference| {
            let TargetData::Object { id } = reference.target;
            Ok(SceneReference {
                source: object_id(&reference.source)?,
                target: ReferenceTarget::Object(object_id(&id)?),
            })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    let fragment = Fragment {
        settings: settings(payload.settings)?,
        roots: payload
            .roots
            .iter()
            .map(|id| object_id(id))
            .collect::<Result<Vec<_>, _>>()?,
        objects,
        references,
    };
    fragment.validate().map_err(domain_refusal)?;
    Ok(fragment)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::ErrorCode;

    fn payload() -> Value {
        json!({
            "format":"chemifree.clipboard","version":2,"kind":"graphics",
            "settings":{"bond_length_pt":30.0,"line_width_pt":1.2,"label_font":"sans-serif","label_weight":"normal","label_style":"normal","label_underline":false,"label_outline":false,"label_shadow":false,
                "label_size_pt":12.0,"carbon_labels":"default","display_unit":"pt"},
            "roots":["s10","s3"],
            "objects":[
                {"kind":"group","id":"s10","children":["s1","s8"]},
                {"kind":"graphic","id":"s8","shape":"arrow","points":[{"x":-10.0,"y":2.0},{"x":-20.0,"y":30.0}]},
                {"kind":"graphic","id":"s3","shape":"rectangle","points":[{"x":40.0,"y":50.0},{"x":20.0,"y":10.0}]},
                {"kind":"graphic","id":"s1","shape":"line","points":[{"x":0.1,"y":-0.0},{"x":30.0,"y":2.0}]}
            ],
            "references":[{"source":"s8","target":{"kind":"object","id":"s3"}}]
        })
    }

    fn refusal(text: &str, reason: &str) {
        let error = decode(text).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidCommand, "{error}");
        assert_eq!(error.reason_code(), Some(reason), "{error}");
    }

    #[test]
    fn round_trip_preserves_forest_order_all_shapes_internal_references_and_float_bits() {
        let expected = payload();
        let fragment = decode(&expected.to_string()).unwrap();
        assert_eq!(
            fragment.roots,
            vec![object_id("s10").unwrap(), object_id("s3").unwrap()]
        );
        assert_eq!(
            fragment
                .objects
                .iter()
                .map(|object| object.id.to_string())
                .collect::<Vec<_>>(),
            vec!["s10", "s8", "s3", "s1"]
        );
        let SceneObject::Graphic(line) = &fragment.objects[3].object else {
            panic!("the fourth fixture object is a line");
        };
        assert_eq!(line.points[0].x().to_bits(), 0.1_f64.to_bits());
        assert_eq!(line.points[0].y().to_bits(), (-0.0_f64).to_bits());
        let encoded = encode(&fragment).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), expected);
        assert_eq!(decode(&encoded).unwrap(), fragment);
    }

    #[test]
    fn older_omitted_style_and_weight_decode_as_normal_and_encode_complete_settings() {
        let current = payload();
        let expected = decode(&current.to_string()).unwrap();
        for omitted in [
            vec!["label_outline"],
            vec!["label_shadow"],
            vec!["label_underline"],
            vec!["label_underline", "label_style", "label_weight"],
            vec!["label_style"],
            vec!["label_weight"],
            vec!["label_style", "label_weight"],
        ] {
            let mut old = current.clone();
            let settings = old["settings"].as_object_mut().unwrap();
            for field in omitted {
                settings.remove(field);
            }
            let decoded = decode(&old.to_string()).unwrap();
            assert_eq!(decoded, expected);
            assert_eq!(
                serde_json::from_str::<Value>(&encode(&decoded).unwrap()).unwrap(),
                current
            );
        }
    }

    #[test]
    fn every_top_level_field_is_required_without_record_defaults() {
        for field in [
            "format",
            "version",
            "kind",
            "settings",
            "roots",
            "objects",
            "references",
        ] {
            let mut invalid = payload();
            invalid.as_object_mut().unwrap().remove(field);
            refusal(&invalid.to_string(), "clipboard_fragment_invalid");
        }
    }

    #[test]
    fn required_nested_fields_and_explicit_coordinates_cannot_be_omitted() {
        for (pointer, field) in [
            ("/settings", "line_width_pt"),
            ("/objects/0", "children"),
            ("/objects/1", "id"),
            ("/objects/1", "kind"),
            ("/objects/1", "shape"),
            ("/objects/1", "points"),
            ("/objects/1/points/0", "x"),
            ("/objects/1/points/0", "y"),
            ("/references/0", "source"),
            ("/references/0", "target"),
            ("/references/0/target", "kind"),
            ("/references/0/target", "id"),
        ] {
            let mut invalid = payload();
            invalid
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            refusal(&invalid.to_string(), "clipboard_fragment_invalid");
        }
    }

    #[test]
    fn unsupported_headers_are_classified_before_any_version_specific_body() {
        for unsupported in [
            json!({"format":"chemifree.clipboard","version":3,"future_body":{"commands":[1,2]}}),
            json!({"format":"chemifree.clipboard","version":u64::MAX,"future":true}),
            json!({"format":"another.format","version":2,"kind":"graphics","unknown":null}),
            json!({"format":"chemifree.clipboard","version":2,"kind":"molecules","new_body":[]}),
            json!({"format":"chemifree.clipboard","version":1,"atoms":[],"bonds":[],"stereo":[]}),
        ] {
            refusal(&unsupported.to_string(), "clipboard_format_unsupported");
        }
    }

    #[test]
    fn malformed_headers_and_non_object_json_are_invalid_not_future_profiles() {
        for invalid in [
            json!(null),
            json!([]),
            json!("chemifree.clipboard"),
            json!({"format":42,"version":2,"kind":"graphics"}),
            json!({"format":"chemifree.clipboard","version":"2","kind":"graphics"}),
            json!({"format":"chemifree.clipboard","version":-1,"kind":"graphics"}),
            json!({"format":"chemifree.clipboard","version":2,"kind":null}),
        ] {
            refusal(&invalid.to_string(), "clipboard_fragment_invalid");
        }
    }

    #[test]
    fn closed_schema_refuses_command_injection_parent_fields_graph_targets_and_bad_tags() {
        for (pointer, field, value) in [
            (
                "",
                "commands",
                json!([{"op":"delete_atoms","atoms":["a1"]}]),
            ),
            ("/objects/0", "parent", json!(null)),
            ("/objects/1", "parent", json!("s10")),
            ("/objects/1/points/0", "z", json!(0)),
            ("/objects/1", "shape", json!("ellipse")),
            ("/objects/1", "kind", json!("instance")),
            ("/references/0/target", "kind", json!("atom")),
            ("/references/0/target", "kind", json!("bond")),
        ] {
            let mut invalid = payload();
            invalid
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(field.into(), value);
            refusal(&invalid.to_string(), "clipboard_fragment_invalid");
        }
    }

    #[test]
    fn duplicate_json_keys_are_rejected_at_every_depth_before_header_dispatch() {
        let text = payload().to_string();
        let duplicate_root = format!("{{\"format\":\"chemifree.clipboard\",{}", &text[1..]);
        refusal(&duplicate_root, "clipboard_fragment_invalid");
        let duplicate_coordinate = text.replacen("\"x\":-10.0", "\"x\":-10.0,\"\\u0078\":-9.0", 1);
        assert_ne!(
            duplicate_coordinate, text,
            "the fixture mutation must add a duplicate coordinate key"
        );
        refusal(&duplicate_coordinate, "clipboard_fragment_invalid");
        refusal(
            r#"{"format":"future","format":"future","version":9}"#,
            "clipboard_fragment_invalid",
        );
    }

    #[test]
    fn noncanonical_object_identifiers_cannot_name_destination_entities_or_commands() {
        for id in ["s0", "s01", "a1", "$0", "s4294967296", "s-1"] {
            for pointer in [
                "/roots/0",
                "/objects/1/id",
                "/objects/0/children/0",
                "/references/0/source",
                "/references/0/target/id",
            ] {
                let mut invalid = payload();
                *invalid.pointer_mut(pointer).unwrap() = json!(id);
                refusal(&invalid.to_string(), "clipboard_fragment_invalid");
            }
        }
    }

    #[test]
    fn finite_scene_coordinates_and_complete_valid_settings_are_required() {
        for (pointer, value) in [
            ("/objects/1/points/0/x", json!(100000.0001)),
            ("/objects/1/points/0/y", json!(-100000.0001)),
            ("/settings/line_width_pt", json!(0.0)),
            ("/settings/bond_length_pt", json!(201.0)),
            ("/settings/label_outline", Value::Null),
            ("/settings/label_outline", json!("true")),
            ("/settings/label_outline", json!(1)),
            ("/settings/label_shadow", Value::Null),
            ("/settings/label_shadow", json!("true")),
            ("/settings/label_shadow", json!(1)),
            ("/settings/label_underline", Value::Null),
            ("/settings/label_underline", json!("true")),
            ("/settings/label_underline", json!(1)),
            ("/settings/label_style", Value::Null),
            ("/settings/label_style", json!("oblique")),
            ("/settings/label_style", json!(500)),
            ("/settings/label_style", json!([])),
            ("/settings/label_style", json!({})),
        ] {
            let mut invalid = payload();
            *invalid.pointer_mut(pointer).unwrap() = value;
            refusal(&invalid.to_string(), "clipboard_fragment_invalid");
        }
        refusal(
            &payload().to_string().replacen("-10.0", "1e309", 1),
            "clipboard_fragment_invalid",
        );
    }

    #[test]
    fn duplicate_structural_ids_and_references_are_rejected_by_domain_validation() {
        let mut duplicate_object = payload();
        let object = duplicate_object["objects"][1].clone();
        duplicate_object["objects"]
            .as_array_mut()
            .unwrap()
            .push(object);
        refusal(
            &duplicate_object.to_string(),
            "clipboard_scene_invalid_tree",
        );
        let mut duplicate_root = payload();
        duplicate_root["roots"] = json!(["s10", "s10", "s3"]);
        refusal(&duplicate_root.to_string(), "clipboard_scene_invalid_tree");
        let mut duplicate_reference = payload();
        let reference = duplicate_reference["references"][0].clone();
        duplicate_reference["references"]
            .as_array_mut()
            .unwrap()
            .push(reference);
        refusal(
            &duplicate_reference.to_string(),
            "clipboard_scene_duplicate_reference",
        );
    }

    #[test]
    fn utf8_limit_accepts_exactly_65536_bytes_and_refuses_a_multibyte_overflow() {
        let text = payload().to_string();
        let at_limit = format!("{text}{}", " ".repeat(MAX_CLIPBOARD_BYTES - text.len()));
        assert_eq!(at_limit.len(), 65536);
        assert_eq!(decode(&at_limit).unwrap(), decode(&text).unwrap());
        let too_long = format!(
            "{text}{}é",
            " ".repeat(MAX_CLIPBOARD_BYTES - 1 - text.len())
        );
        assert_eq!(too_long.len(), 65537);
        refusal(&too_long, "clipboard_byte_limit");
    }

    #[test]
    fn encode_checks_output_bytes_after_validating_an_in_budget_forest() {
        let mut fragment = decode(&payload().to_string()).unwrap();
        fragment.roots.clear();
        fragment.objects.clear();
        fragment.references.clear();
        for serial in 1..=500 {
            let id = ObjectId::from_serial(serial).unwrap();
            fragment.roots.push(id);
            fragment.objects.push(FragmentObject {
                id,
                object: SceneObject::Graphic(Graphic {
                    line_width_pt: None,
                    shape: GraphicShape::Line,
                    points: [
                        Point::new(12345.67890123456, -12345.67890123456).unwrap(),
                        Point::new(-12345.67890123456, 12345.67890123456).unwrap(),
                    ],
                }),
            });
        }
        fragment.validate().unwrap();
        let error = encode(&fragment).unwrap_err();
        assert_eq!(error.reason_code(), Some("clipboard_byte_limit"));
    }

    #[test]
    fn graph_version_one_remains_exclusive_to_existing_graph_codec() {
        let graph = json!({"format":"chemifree.clipboard","version":1,"settings":payload()["settings"],
            "atoms":[{"id":"a1","kind":{"type":"element","symbol":"C"},"charge":0,"isotope":null,
                "hydrogens":{"mode":"inferred"},"radical":"none","attachment_points":[],"position":{"x":0.0,"y":0.0}}],
            "bonds":[],"stereo":[]}).to_string();
        assert_eq!(crate::clipboard::decode(&graph).unwrap().atoms.len(), 1);
        refusal(&graph, "clipboard_format_unsupported");
        let error = crate::clipboard::decode(&payload().to_string()).unwrap_err();
        assert_eq!(error.reason_code(), Some("clipboard_format_unsupported"));
    }
}
