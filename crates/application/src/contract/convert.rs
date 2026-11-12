//! Conversion of wire commands to domain commands.

use chemifree_domain::document::{
    Atom, AtomEdit, BondTo, Command, FragmentDisplayRun, MAX_CHARGE, MIN_CHARGE, NewAtomPositions,
    Vector,
};
use chemifree_domain::image::{Image, ImageSize, PngCache, PngDecoder};
use chemifree_domain::scene::{CaptionScript, DeleteMode, Graphic, GraphicShape, SceneError};
use chemifree_domain::states::{Radical, StateError};
use chemifree_domain::stereo::StereoMark;

use crate::error::{ApplicationError, ErrorCode};

use super::command::CommandDto;
use super::parse::{
    Refusal, atom_configuration, atom_id, atom_kind, atom_ref, atom_refs, attachment_points,
    bond_configuration, bond_id, bond_order, bond_ref, caption, exact_selection, group_number,
    group_ref, instance_id, point, reference, to_hydrogens, vector,
};
use super::scene::{CaptionScriptDto, DeleteModeDto, GraphicShapeDto};
use super::settings::{SettingsEditDto, to_settings_edit};

/// Converts wire commands to domain commands, reporting the first malformed field.
pub(crate) fn to_commands(
    commands: &[CommandDto],
    decoder: Option<&dyn PngDecoder>,
    cache: &mut PngCache,
) -> Result<Vec<Command>, ApplicationError> {
    commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            to_command(command, decoder, cache).map_err(|refusal| ApplicationError {
                command_index: Some(index),
                invalid_state: refusal.state,
                invalid_scene: refusal.scene,
                invalid_stereo_group: refusal.group,
                ..ApplicationError::new(
                    ErrorCode::InvalidCommand,
                    format!("command {index}: {}", refusal.message),
                )
            })
        })
        .collect()
}

fn to_command(
    command: &CommandDto,
    decoder: Option<&dyn PngDecoder>,
    cache: &mut PngCache,
) -> Result<Command, Refusal> {
    Ok(match command {
        CommandDto::InsertRing {
            instance,
            center,
            size,
            pattern,
        } => Command::InsertRing {
            instance: instance_id(instance)?,
            center: point(*center)?,
            size: *size,
            pattern: *pattern,
        },
        CommandDto::AttachRing {
            instance,
            atom,
            direction,
            size,
            pattern,
        } => Command::AttachRing {
            instance: instance_id(instance)?,
            atom: atom_id(atom)?,
            direction: Vector {
                dx: direction.dx,
                dy: direction.dy,
            },
            size: *size,
            pattern: *pattern,
        },
        CommandDto::AddAtom {
            element: symbol,
            query,
            position,
            positions,
            bond_to,
            hydrogens,
            radical,
            attachment_points: points,
        } => Command::AddAtom {
            atom: Atom {
                hydrogens: hydrogens.map(to_hydrogens).transpose()?.unwrap_or_default(),
                radical: radical.map(Radical::from).unwrap_or_default(),
                attachment_points: points
                    .as_deref()
                    .map(attachment_points)
                    .transpose()?
                    .unwrap_or_default(),
                ..Atom::new(
                    atom_kind(symbol.as_deref(), query.as_ref())?
                        .ok_or(StateError::AtomKindConflict)?,
                )
            },
            positions: match (position, positions) {
                (Some(position), None) => NewAtomPositions::Single(point(*position)?),
                (None, Some(positions)) => NewAtomPositions::PerInstance(
                    positions
                        .iter()
                        .map(|(id, position)| Ok((instance_id(id)?, point(*position)?)))
                        .collect::<Result<_, String>>()?,
                ),
                (Some(_), Some(_)) => {
                    return Err("give either position or positions, not both"
                        .to_owned()
                        .into());
                }
                (None, None) => {
                    return Err("add_atom needs position or positions".to_owned().into());
                }
            },
            bond_to: bond_to
                .as_ref()
                .map(|target| {
                    Ok::<_, Refusal>(BondTo {
                        atom: atom_ref(&target.atom)?,
                        order: bond_order(target.order, target.query.as_ref())?,
                    })
                })
                .transpose()?,
        },
        CommandDto::AddBond {
            begin,
            end,
            order,
            query,
        } => Command::AddBond {
            begin: atom_ref(begin)?,
            end: atom_ref(end)?,
            order: bond_order(*order, query.as_ref())?,
        },
        CommandDto::UpdateAtom {
            atom,
            element: symbol,
            query,
            charge,
            isotope,
            hydrogens,
            radical,
            attachment_points: points,
        } => Command::UpdateAtom {
            atom: atom_id(atom)?,
            edit: AtomEdit {
                kind: atom_kind(symbol.as_deref(), query.as_ref())?,
                // The domain checks the charge range; this only narrows the wire integer.
                charge: charge
                    .map(|charge| {
                        i8::try_from(charge).map_err(|_| {
                            format!("charge {charge} is outside {MIN_CHARGE}..={MAX_CHARGE}")
                        })
                    })
                    .transpose()?,
                isotope: isotope
                    .map(|isotope| {
                        isotope
                            .map(|mass| {
                                u16::try_from(mass)
                                    .map_err(|_| format!("isotope mass number {mass} is too large"))
                            })
                            .transpose()
                    })
                    .transpose()?,
                hydrogens: hydrogens.map(to_hydrogens).transpose()?,
                radical: radical.map(Radical::from),
                attachment_points: points.as_deref().map(attachment_points).transpose()?,
            },
        },
        CommandDto::MoveAtoms {
            atoms,
            delta,
            instance,
        } => Command::MoveAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            delta: vector(*delta),
            instance: instance.as_deref().map(instance_id).transpose()?,
        },
        CommandDto::MoveSelection {
            selection,
            delta,
            instance,
        } => Command::MoveSelection {
            selection: exact_selection(selection).map_err(|message| Refusal {
                scene: Some(SceneError::SelectionUnknown),
                ..message.into()
            })?,
            instance: instance.as_deref().map(instance_id).transpose()?,
            delta: vector(*delta),
        },
        CommandDto::ArrangeObjects { objects, mode } => Command::ArrangeObjects {
            objects: objects
                .iter()
                .map(|object| instance_id(object))
                .collect::<Result<_, _>>()?,
            arrangement: (*mode).into(),
        },
        CommandDto::TransformSelection {
            selection,
            pivot,
            transform,
            instance,
        } => Command::TransformSelection {
            selection: exact_selection(selection).map_err(|message| Refusal {
                scene: Some(SceneError::SelectionUnknown),
                ..message.into()
            })?,
            instance: instance.as_deref().map(instance_id).transpose()?,
            pivot: point(*pivot)?,
            transform: (*transform).into(),
        },
        CommandDto::UpdateBond { bond, order, query } => Command::UpdateBond {
            bond: bond_id(bond)?,
            order: bond_order(*order, query.as_ref())?,
        },
        CommandDto::DeleteAtoms { atoms } => Command::DeleteAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::DeleteBonds { bonds } => Command::DeleteBonds {
            bonds: bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::Rename { title } => Command::Rename {
            title: title.clone(),
        },
        CommandDto::AddInstance { source, offset } => Command::AddInstance {
            source: source.as_deref().map(instance_id).transpose()?,
            offset: vector(*offset),
        },
        CommandDto::MoveInstance { instance, offset } => Command::MoveInstance {
            instance: instance_id(instance)?,
            offset: vector(*offset),
        },
        CommandDto::DeleteInstance { instance } => Command::DeleteInstance {
            instance: instance_id(instance)?,
        },
        CommandDto::ContractFragment {
            instance,
            anchor,
            members,
            text,
        } => Command::ContractFragment {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
            members: members
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            text: text.clone(),
        },
        CommandDto::SetFragmentDisplayText {
            instance,
            anchor,
            text,
        } => Command::SetFragmentDisplayText {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
            text: text.clone(),
        },
        CommandDto::SetFragmentDisplayRuns {
            instance,
            anchor,
            runs,
        } => Command::SetFragmentDisplayRuns {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
            runs: runs
                .iter()
                .map(|run| FragmentDisplayRun {
                    text: run.text.clone(),
                    script: match run.script {
                        CaptionScriptDto::Baseline => CaptionScript::Baseline,
                        CaptionScriptDto::Subscript => CaptionScript::Subscript,
                        CaptionScriptDto::Superscript => CaptionScript::Superscript,
                    },
                })
                .collect(),
        },
        CommandDto::ExpandFragment { instance, anchor } => Command::ExpandFragment {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
        },
        CommandDto::CollapseEthyl {
            instance,
            anchor,
            terminal,
        } => Command::CollapseEthyl {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
            terminal: atom_id(terminal)?,
        },
        CommandDto::UpdateAbbreviationLabel {
            instance,
            anchor,
            text,
        } => Command::UpdateAbbreviationLabel {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
            text: text.clone(),
        },
        CommandDto::ExpandEthyl { instance, anchor } => Command::ExpandEthyl {
            instance: instance_id(instance)?,
            anchor: atom_id(anchor)?,
        },
        CommandDto::AddCaption {
            alignment,
            anchor,
            runs,
            parent,
            index,
        } => Command::AddCaption {
            caption: caption(*anchor, *alignment, runs)?,
            parent: parent.as_deref().map(instance_id).transpose()?,
            index: *index,
        },
        CommandDto::UpdateCaption {
            alignment,
            object,
            anchor,
            runs,
        } => Command::UpdateCaption {
            object: instance_id(object)?,
            caption: caption(*anchor, *alignment, runs)?,
        },
        CommandDto::AddImage {
            mime_type,
            data_base64,
            origin,
            size_pt,
            parent,
            index,
        } => {
            let invalid = |message: String| Refusal {
                message,
                state: None,
                scene: Some(SceneError::ImageInvalid),
                group: None,
            };
            if mime_type != "image/png" {
                return Err(invalid("Only embedded image/png is supported".into()));
            }
            let decoder =
                decoder.ok_or_else(|| invalid("Embedded PNG decoder is not configured".into()))?;
            let asset = cache.decode(data_base64, decoder).map_err(invalid)?;
            let size = ImageSize::new(size_pt.x, size_pt.y).map_err(|e| invalid(e.to_string()))?;
            let image =
                Image::new(asset, point(*origin)?, size).map_err(|e| invalid(e.to_string()))?;
            Command::AddImage {
                image,
                parent: parent.as_deref().map(instance_id).transpose()?,
                index: *index,
            }
        }
        CommandDto::UpdateImageGeometry {
            object,
            origin,
            size_pt,
        } => Command::UpdateImageGeometry {
            object: instance_id(object)?,
            origin: point(*origin)?,
            size_pt: ImageSize::new(size_pt.x, size_pt.y).map_err(|e| Refusal {
                message: e.to_string(),
                state: None,
                scene: Some(SceneError::ImageInvalid),
                group: None,
            })?,
        },
        CommandDto::AddGraphic {
            shape,
            points,
            parent,
            index,
        } => Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape: match shape {
                    GraphicShapeDto::Line => GraphicShape::Line,
                    GraphicShapeDto::Rectangle => GraphicShape::Rectangle,
                    GraphicShapeDto::Ellipse => GraphicShape::Ellipse,
                    GraphicShapeDto::Arrow => GraphicShape::Arrow,
                    GraphicShapeDto::Polygon => GraphicShape::Polygon,
                    GraphicShapeDto::Spline => GraphicShape::Spline,
                },
                points: vec![point(points[0])?, point(points[1])?],
            },
            parent: parent.as_deref().map(instance_id).transpose()?,
            index: *index,
        },
        CommandDto::SetGraphicLineWidth {
            object,
            line_width_pt,
        } => Command::SetGraphicLineWidth {
            object: instance_id(object)?,
            line_width_pt: *line_width_pt,
        },
        CommandDto::AddSpline {
            points,
            parent,
            index,
        } => Command::AddSpline {
            points: [
                point(points[0])?,
                point(points[1])?,
                point(points[2])?,
                point(points[3])?,
            ],
            line_width_pt: None,
            parent: parent.as_deref().map(instance_id).transpose()?,
            index: *index,
        },
        CommandDto::UpdateSplinePoints { object, points } => Command::UpdateSplinePoints {
            object: instance_id(object)?,
            points: [
                point(points[0])?,
                point(points[1])?,
                point(points[2])?,
                point(points[3])?,
            ],
        },
        CommandDto::AddPolygon {
            vertices,
            parent,
            index,
        } => Command::AddPolygon {
            vertices: vertices
                .iter()
                .copied()
                .map(point)
                .collect::<Result<_, _>>()?,
            line_width_pt: None,
            parent: parent.as_deref().map(instance_id).transpose()?,
            index: *index,
        },
        CommandDto::UpdatePolygonVertices { object, vertices } => Command::UpdatePolygonVertices {
            object: instance_id(object)?,
            vertices: vertices
                .iter()
                .copied()
                .map(point)
                .collect::<Result<_, _>>()?,
        },
        CommandDto::UpdateGraphicPoints { object, points } => Command::UpdateGraphicPoints {
            object: instance_id(object)?,
            points: [point(points[0])?, point(points[1])?],
        },
        CommandDto::Group { objects } => Command::Group {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::Ungroup { group } => Command::Ungroup {
            group: instance_id(group)?,
        },
        CommandDto::Reorder { object, index } => Command::Reorder {
            object: instance_id(object)?,
            index: *index,
        },
        CommandDto::DeleteObjects { objects, mode } => Command::DeleteObjects {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
            mode: match mode.ok_or_else(|| Refusal {
                message: "delete_objects requires a mode".to_owned(),
                state: None,
                scene: Some(SceneError::DeleteModeMissing),
                group: None,
            })? {
                DeleteModeDto::Subtree => DeleteMode::Subtree,
                DeleteModeDto::ObjectOnly => DeleteMode::ObjectOnly,
            },
        },
        CommandDto::MoveObjects { objects, delta } => Command::MoveObjects {
            objects: objects
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
            delta: vector(*delta),
        },
        CommandDto::AddReference { source, target } => Command::AddReference {
            reference: reference(source, target)?,
        },
        CommandDto::RemoveReference { source, target } => Command::RemoveReference {
            reference: reference(source, target)?,
        },
        CommandDto::UpdateSettings {
            bond_length_pt,
            line_width_pt,
            label_font,
            label_weight,
            label_style,
            label_underline,
            label_outline,
            label_shadow,
            label_size_pt,
            carbon_labels,
            display_unit,
        } => Command::UpdateSettings(to_settings_edit(&SettingsEditDto {
            bond_length_pt: *bond_length_pt,
            line_width_pt: *line_width_pt,
            label_font: *label_font,
            label_weight: *label_weight,
            label_style: *label_style,
            label_underline: *label_underline,
            label_outline: *label_outline,
            label_shadow: *label_shadow,
            label_size_pt: *label_size_pt,
            carbon_labels: *carbon_labels,
            display_unit: *display_unit,
        })),
        CommandDto::SetBondMark { bond, mark } => Command::SetBondMark {
            bond: bond_ref(bond)?,
            mark: mark.as_ref().map(StereoMark::from),
        },
        CommandDto::SetAtomConfiguration {
            atom,
            configuration,
        } => Command::SetAtomConfiguration {
            atom: atom_ref(atom)?,
            configuration: configuration.as_ref().map(atom_configuration).transpose()?,
        },
        CommandDto::SetBondConfiguration {
            bond,
            configuration,
        } => Command::SetBondConfiguration {
            bond: bond_ref(bond)?,
            configuration: configuration.as_ref().map(bond_configuration).transpose()?,
        },
        CommandDto::SetConfigurationFromDrawing {
            atoms,
            bonds,
            instance,
        } => Command::SetConfigurationFromDrawing {
            atoms: atoms
                .as_ref()
                .map(|atoms| atoms.iter().map(|id| atom_ref(id)).collect())
                .transpose()?,
            bonds: bonds
                .as_ref()
                .map(|bonds| bonds.iter().map(|id| bond_ref(id)).collect())
                .transpose()?,
            instance: instance.as_deref().map(instance_id).transpose()?,
        },
        CommandDto::CreateStereoGroup {
            kind,
            atoms,
            number,
        } => Command::CreateStereoGroup {
            kind: (*kind).into(),
            number: group_number(*number)?,
            atoms: atom_refs(atoms)?,
        },
        CommandDto::DeleteStereoGroup { group } => Command::DeleteStereoGroup {
            group: group_ref(group)?,
        },
        CommandDto::SetStereoGroupKind { group, kind } => Command::SetStereoGroupKind {
            group: group_ref(group)?,
            kind: (*kind).into(),
        },
        CommandDto::SetStereoGroupNumber { group, number } => Command::SetStereoGroupNumber {
            group: group_ref(group)?,
            number: group_number(*number)?,
        },
        CommandDto::AddStereoGroupMembers { group, atoms } => Command::AddStereoGroupMembers {
            group: group_ref(group)?,
            atoms: atom_refs(atoms)?,
        },
        CommandDto::RemoveStereoGroupMembers { group, atoms } => {
            Command::RemoveStereoGroupMembers {
                group: group_ref(group)?,
                atoms: atom_refs(atoms)?,
            }
        }
        CommandDto::MergeStereoGroups { into, from } => Command::MergeStereoGroups {
            into: group_ref(into)?,
            from: group_ref(from)?,
        },
        CommandDto::SplitStereoGroup {
            group,
            atoms,
            number,
        } => Command::SplitStereoGroup {
            group: group_ref(group)?,
            atoms: atom_refs(atoms)?,
            number: group_number(*number)?,
        },
    })
}
