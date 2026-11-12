//! Codecs for scene objects (captions, graphics, images), references, selection and
//! fragment displays.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    AtomId, FragmentDisplay, FragmentDisplayRun, MOLECULE_ID, PAGE_ID,
};
use chemifree_domain::image::{
    Image, ImageSize, MAX_CURRENT_IMAGE_PIXELS, MAX_HISTORY_IMAGE_PIXELS, PngCache,
};
use chemifree_domain::scene::{
    Caption, CaptionAlignment, CaptionRun, CaptionScript, CaptionStyle, CaptionWeight, Graphic,
    GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
};

use super::graph::{atom_id, bond_id, instance_id, parents, point, stored_point};
use super::records::StoredPoint;
use super::scene::{
    SceneState, StoredCaptionAlignment, StoredCaptionRun, StoredCaptionScript, StoredCaptionStyle,
    StoredCaptionWeight, StoredFragmentDisplayRun, StoredFragmentDisplayState, StoredReference,
    StoredReferenceTarget, StoredSelection, StoredShape,
};

pub(super) fn stored_fragment_display(display: &FragmentDisplay) -> StoredFragmentDisplayState {
    StoredFragmentDisplayState {
        members: display.members.iter().map(ToString::to_string).collect(),
        text: None,
        runs: Some(
            display
                .runs
                .iter()
                .map(|run| StoredFragmentDisplayRun {
                    text: run.text.clone(),
                    script: match run.script {
                        CaptionScript::Baseline => StoredCaptionScript::Baseline,
                        CaptionScript::Subscript => StoredCaptionScript::Subscript,
                        CaptionScript::Superscript => StoredCaptionScript::Superscript,
                    },
                })
                .collect(),
        ),
    }
}

pub(super) fn read_fragment_display(
    display: StoredFragmentDisplayState,
) -> Result<FragmentDisplay, String> {
    let mut members: Vec<AtomId> = display
        .members
        .iter()
        .map(|id| atom_id(id))
        .collect::<Result<_, _>>()?;
    // Lists may arrive in any order. Sorting preserves duplicates for domain rejection
    // and makes current/history sides compare by the same canonical representation.
    members.sort_unstable();
    match (display.text, display.runs) {
        (Some(text), None) => Ok(FragmentDisplay::plain(members, text)),
        (None, Some(runs)) => Ok(FragmentDisplay {
            members,
            runs: runs
                .into_iter()
                .map(|run| FragmentDisplayRun {
                    text: run.text,
                    script: match run.script {
                        StoredCaptionScript::Baseline => CaptionScript::Baseline,
                        StoredCaptionScript::Subscript => CaptionScript::Subscript,
                        StoredCaptionScript::Superscript => CaptionScript::Superscript,
                    },
                })
                .collect(),
        }),
        _ => Err(
            "Fragment display requires exactly one version-appropriate text representation.".into(),
        ),
    }
}

pub(super) fn stored_scene_state(object: &SceneObject) -> SceneState {
    match object {
        SceneObject::Caption(caption) => SceneState::Caption {
            alignment: Some(match caption.alignment {
                CaptionAlignment::Left => StoredCaptionAlignment::Left,
                CaptionAlignment::Center => StoredCaptionAlignment::Center,
                CaptionAlignment::Right => StoredCaptionAlignment::Right,
            }),
            anchor: stored_point(&caption.anchor),
            runs: caption
                .runs
                .iter()
                .map(|run| StoredCaptionRun {
                    shadow: run.shadow,
                    outline: run.outline,
                    underline: run.underline,
                    color: run.color.clone(),
                    size_pt: run.size_pt,
                    font: run.font.map(Into::into),
                    text: run.text.clone(),
                    style: Some(match run.style {
                        CaptionStyle::Normal => StoredCaptionStyle::Normal,
                        CaptionStyle::Italic => StoredCaptionStyle::Italic,
                    }),
                    weight: Some(match run.weight {
                        CaptionWeight::Normal => StoredCaptionWeight::Normal,
                        CaptionWeight::Bold => StoredCaptionWeight::Bold,
                    }),
                    script: match run.script {
                        CaptionScript::Baseline => StoredCaptionScript::Baseline,
                        CaptionScript::Subscript => StoredCaptionScript::Subscript,
                        CaptionScript::Superscript => StoredCaptionScript::Superscript,
                    },
                })
                .collect(),
        },
        SceneObject::Instance => SceneState::Instance {
            molecule: MOLECULE_ID.to_owned(),
        },
        SceneObject::Group { children } => SceneState::Group {
            children: children.iter().map(ToString::to_string).collect(),
        },
        SceneObject::Image(image) => SceneState::Image {
            mime_type: "image/png".into(),
            data_base64: image.asset.data_base64().into(),
            origin: stored_point(&image.origin),
            size_pt: StoredPoint {
                x: image.size_pt.x(),
                y: image.size_pt.y(),
            },
        },
        SceneObject::Graphic(graphic) => SceneState::Graphic {
            line_width_pt: graphic.line_width_pt,
            shape: match graphic.shape {
                GraphicShape::Line => StoredShape::Line,
                GraphicShape::Rectangle => StoredShape::Rectangle,
                GraphicShape::Ellipse => StoredShape::Ellipse,
                GraphicShape::Arrow => StoredShape::Arrow,
                GraphicShape::Polygon => StoredShape::Polygon,
                GraphicShape::Spline => StoredShape::Spline,
            },
            points: graphic.points.iter().map(stored_point).collect(),
        },
    }
}

pub(super) fn stored_reference(reference: &SceneReference) -> StoredReference {
    let target = match reference.target {
        ReferenceTarget::Object(id) => StoredReferenceTarget::Object { id: id.to_string() },
        ReferenceTarget::Atom(id) => StoredReferenceTarget::Atom { id: id.to_string() },
        ReferenceTarget::Bond(id) => StoredReferenceTarget::Bond { id: id.to_string() },
    };
    StoredReference {
        source: reference.source.to_string(),
        target,
    }
}

pub(super) fn read_reference(reference: StoredReference) -> Result<SceneReference, String> {
    let target = match reference.target {
        StoredReferenceTarget::Object { id } => ReferenceTarget::Object(instance_id(&id)?),
        StoredReferenceTarget::Atom { id } => ReferenceTarget::Atom(atom_id(&id)?),
        StoredReferenceTarget::Bond { id } => ReferenceTarget::Bond(bond_id(&id)?),
    };
    Ok(SceneReference {
        source: instance_id(&reference.source)?,
        target,
    })
}

pub(super) fn read_scene_state(
    object: SceneState,
    image_cache: &mut PngCache,
) -> Result<SceneObject, String> {
    Ok(match object {
        SceneState::Caption {
            alignment,
            anchor,
            runs,
        } => {
            let caption = Caption {
                alignment: match alignment.unwrap_or(StoredCaptionAlignment::Left) {
                    StoredCaptionAlignment::Left => CaptionAlignment::Left,
                    StoredCaptionAlignment::Center => CaptionAlignment::Center,
                    StoredCaptionAlignment::Right => CaptionAlignment::Right,
                },
                anchor: point(anchor.x, anchor.y)?,
                runs: runs
                    .into_iter()
                    .map(|run| CaptionRun {
                        shadow: run.shadow,
                        outline: run.outline,
                        underline: run.underline,
                        color: run.color,
                        size_pt: run.size_pt,
                        font: run.font.map(Into::into),
                        text: run.text,
                        style: match run.style.unwrap_or(StoredCaptionStyle::Normal) {
                            StoredCaptionStyle::Normal => CaptionStyle::Normal,
                            StoredCaptionStyle::Italic => CaptionStyle::Italic,
                        },
                        weight: match run.weight.unwrap_or(StoredCaptionWeight::Normal) {
                            StoredCaptionWeight::Normal => CaptionWeight::Normal,
                            StoredCaptionWeight::Bold => CaptionWeight::Bold,
                        },
                        script: match run.script {
                            StoredCaptionScript::Baseline => CaptionScript::Baseline,
                            StoredCaptionScript::Subscript => CaptionScript::Subscript,
                            StoredCaptionScript::Superscript => CaptionScript::Superscript,
                        },
                    })
                    .collect(),
            };
            caption.validate().map_err(|cause| cause.to_string())?;
            SceneObject::Caption(caption)
        }
        SceneState::Instance { molecule } => {
            parents(PAGE_ID, &molecule)?;
            SceneObject::Instance
        }
        SceneState::Group { children } => SceneObject::Group {
            children: children
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
        },
        SceneState::Image {
            mime_type,
            data_base64,
            origin,
            size_pt,
        } => {
            if mime_type != "image/png" {
                return Err("Only embedded image/png is supported".into());
            }
            let asset = image_cache.decode(&data_base64, &crate::png_image::EmbeddedPngDecoder)?;
            if image_cache.live_pixels() > MAX_CURRENT_IMAGE_PIXELS + MAX_HISTORY_IMAGE_PIXELS {
                return Err(
                    "Incoming unique PNG assets exceed the native pixel validation budget".into(),
                );
            }
            SceneObject::Image(
                Image::new(
                    asset,
                    point(origin.x, origin.y)?,
                    ImageSize::new(size_pt.x, size_pt.y).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?,
            )
        }
        SceneState::Graphic {
            shape,
            points,
            line_width_pt,
        } => SceneObject::Graphic(Graphic {
            line_width_pt,
            shape: match shape {
                StoredShape::Line => GraphicShape::Line,
                StoredShape::Rectangle => GraphicShape::Rectangle,
                StoredShape::Ellipse => GraphicShape::Ellipse,
                StoredShape::Arrow => GraphicShape::Arrow,
                StoredShape::Polygon => GraphicShape::Polygon,
                StoredShape::Spline => GraphicShape::Spline,
            },
            points: points
                .into_iter()
                .map(|p| point(p.x, p.y))
                .collect::<Result<_, _>>()?,
        }),
    })
}

pub(super) fn stored_selection(selection: &Selection) -> StoredSelection {
    StoredSelection {
        objects: selection.objects.iter().map(ToString::to_string).collect(),
        atoms: selection.atoms.iter().map(ToString::to_string).collect(),
        bonds: selection.bonds.iter().map(ToString::to_string).collect(),
    }
}

pub(super) fn read_selection(selection: StoredSelection) -> Result<Selection, String> {
    fn distinct<T: Ord>(
        values: Vec<String>,
        parse: fn(&str) -> Result<T, String>,
    ) -> Result<BTreeSet<T>, String> {
        let mut parsed = BTreeSet::new();
        for value in values {
            if !parsed.insert(parse(&value)?) {
                return Err(format!("selection repeats identifier {value:?}"));
            }
        }
        Ok(parsed)
    }
    Ok(Selection {
        objects: distinct(selection.objects, instance_id)?,
        atoms: distinct(selection.atoms, atom_id)?,
        bonds: distinct(selection.bonds, bond_id)?,
    })
}
