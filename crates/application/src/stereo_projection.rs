//! The format-neutral half of projecting drawn stereo: deriving stored configurations from marks
//! and positions, comparing stored configurations with the drawing a written file would show,
//! and mapping a file's stereo groups to the document's group commands. A codec supplies what
//! differs per format (how a written position is rounded, its own paths, reason codes and
//! wording); nothing here names a file format.

use std::collections::{BTreeSet, HashMap, HashSet};

use chemifree_domain::document::{
    AtomId, AtomRef, Command, Document, DocumentId, GroupId, GroupRef, Point,
};
use chemifree_domain::stereo::{CenterState, StereoKey, StereoState};
use chemifree_domain::stereo_drawing::Drawing;
use chemifree_domain::stereo_group::GroupKind;

use crate::contract::{DocumentView, StereoGroupKindDto};
use crate::{Loss, LossConsent, LossDisposition, SemanticImpact};

/// Whether a derived center is a stored tetrahedral configuration (a wavy mark derives
/// `unknown`, which no group can hold). The one predicate of the import's member check and the
/// export's refusal, so a file this writes is one this reader accepts.
pub(crate) fn is_tetrahedral(state: &CenterState) -> bool {
    matches!(state, CenterState::Tetrahedral { .. })
}

/// The group members whose written depiction would not derive a tetrahedral configuration:
/// derived on the positions and marks as the file will hold them (the same drawing the consent
/// comparison reads), `written` being the format's rounding of a position. A V3000 file with
/// such a member is refused by this reader, so the export refuses instead of writing it;
/// consent cannot cover it.
pub(crate) fn undrawn_group_members(
    document: &Document,
    view: &DocumentView,
    written: impl Fn(Point) -> Point,
) -> Vec<String> {
    let drawing = Drawing::of_document_shown(document, document.first_instance(), written);
    let mut undrawn = Vec::new();
    for group in &view.stereo.groups {
        for member in &group.members {
            let derives = AtomId::parse(member).is_some_and(|atom| {
                drawing
                    .derive_center(atom)
                    .is_ok_and(|s| is_tetrahedral(&s))
            });
            if !derives {
                undrawn.push(member.clone());
            }
        }
    }
    undrawn
}

fn dto_kind(kind: StereoGroupKindDto) -> GroupKind {
    kind.into()
}

/// The smallest number from 1 up that `used` does not hold; `None` only if every positive
/// 32-bit integer is in use. Deterministic, and never a number already in use.
fn free_number(used: &BTreeSet<i32>) -> Option<i32> {
    (1..=i32::MAX).find(|candidate| !used.contains(candidate))
}

/// One stereo group a file states: its kind, the number the file gave it (`None` when it states
/// none) and its members as positions among the atoms the import created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GroupInput {
    pub(crate) kind: GroupKind,
    pub(crate) number: Option<i32>,
    pub(crate) atoms: Vec<usize>,
}

/// Why a file's groups cannot be mapped; the codec words it with its own reason code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupFault {
    /// The marks could not be read into configurations at all.
    DrawingUnreadable,
    /// Group `group` (0-based) lists atom `atom` (0-based), which derived no tetrahedral
    /// configuration.
    MemberNotStereocenter { group: usize, atom: usize },
    /// No positive group number is left to renumber a colliding group.
    NoFreeNumber,
}

fn stereo_loss(source_path: String, feature: &str, reason: &str) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path,
        disposition: LossDisposition::Normalized,
        reason: reason.into(),
        semantic_impact: SemanticImpact::Stereo,
    }
}

/// The commands that turn the file's groups into stereo groups, after the configurations they
/// refer to. A member must be an atom whose marks derived a tetrahedral configuration; one that
/// is not refuses the whole record, because dropping it would change which centers the file
/// says belong together. Appending to a document that has groups never merges two numbered
/// groups and never drops a member: an absolute group joins the document's absolute group and
/// an `and` or `or` number the document already uses is replaced by a free one; both are
/// reported, at `group_path(k)` for the k-th (0-based) group.
pub(crate) fn group_commands(
    groups: &[GroupInput],
    doc: &DocumentView,
    tetrahedral: Option<&HashSet<usize>>,
    group_path: impl Fn(usize) -> String,
    losses: &mut Vec<Loss>,
) -> Result<Vec<Command>, GroupFault> {
    let mut commands = Vec::with_capacity(groups.len());
    let Some(tetrahedral) = tetrahedral else {
        // The marks could not be read into configurations at all (a `stereo_unreadable` loss
        // says why); the members are not at fault, so the cause is named, not the atoms.
        return if groups.is_empty() {
            Ok(commands)
        } else {
            Err(GroupFault::DrawingUnreadable)
        };
    };
    let existing_absolute = doc
        .stereo
        .groups
        .iter()
        .find(|group| dto_kind(group.kind) == GroupKind::Absolute)
        .and_then(|group| GroupId::parse(&group.id));
    let in_document = |kind: GroupKind| -> BTreeSet<i32> {
        doc.stereo
            .groups
            .iter()
            .filter(|group| dto_kind(group.kind) == kind)
            .filter_map(|group| group.number)
            .collect()
    };
    // Computed once per kind: the numbers the document uses (a group that stated one of them is
    // renumbered), and those plus every number the file states or has been given so far, so a
    // free number never meets another group of the file.
    let mut used: HashMap<GroupKind, BTreeSet<i32>> = HashMap::new();
    let mut reserved: HashMap<GroupKind, BTreeSet<i32>> = HashMap::new();
    for kind in [GroupKind::And, GroupKind::Or] {
        let numbers = in_document(kind);
        let mut all = numbers.clone();
        all.extend(
            groups
                .iter()
                .filter(|group| group.kind == kind)
                .map(|group| group.number.unwrap_or(0)),
        );
        used.insert(kind, numbers);
        reserved.insert(kind, all);
    }
    for (k, group) in groups.iter().enumerate() {
        let path = group_path(k);
        let mut atoms = Vec::with_capacity(group.atoms.len());
        for &position in &group.atoms {
            if !tetrahedral.contains(&position) {
                return Err(GroupFault::MemberNotStereocenter {
                    group: k,
                    atom: position,
                });
            }
            atoms.push(AtomRef::Created(position));
        }
        let kind = group.kind;
        if kind == GroupKind::Absolute {
            if let Some(existing) = existing_absolute {
                losses.push(stereo_loss(
                    path,
                    "stereo_group_merged",
                    "The document already has an absolute group; the file's absolute members joined it, so they now state the same thing as the document's own.",
                ));
                commands.push(Command::AddStereoGroupMembers {
                    group: GroupRef::Id(existing),
                    atoms,
                });
            } else {
                commands.push(Command::CreateStereoGroup {
                    kind,
                    number: None,
                    atoms,
                });
            }
            continue;
        }
        let stated = group.number.unwrap_or(0);
        let number = if used
            .get(&kind)
            .is_some_and(|numbers| numbers.contains(&stated))
        {
            let numbers = reserved.entry(kind).or_default();
            let free = free_number(numbers).ok_or(GroupFault::NoFreeNumber)?;
            numbers.insert(free);
            losses.push(stereo_loss(
                path,
                "stereo_group_number",
                &format!(
                    "The document already has a {} group numbered {stated}; the file's group is numbered {free} instead and stays a separate group.",
                    kind.as_str()
                ),
            ));
            free
        } else {
            stated
        };
        commands.push(Command::CreateStereoGroup {
            kind,
            number: Some(number),
            atoms,
        });
    }
    Ok(commands)
}

/// A loss of the import's derivation: `stereo_unreadable`, dropped, at `source_path`.
fn unreadable_loss(source_path: String, reason: &str) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: "stereo_unreadable".into(),
        source_path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: SemanticImpact::Stereo,
    }
}

/// No configuration could be derived for the record at all (the scratch drawing was refused):
/// the marks stay and one loss says why, so the import never silently stores less.
fn unreadable_record(
    prefix: &str,
    cause: &str,
    losses: &mut Vec<Loss>,
) -> (Option<Command>, Option<HashSet<usize>>) {
    losses.push(unreadable_loss(
        format!("{prefix}.atoms"),
        &format!("The drawn marks could not be read into configurations ({cause}); the marks are kept and no configuration is stored."),
    ));
    (None, None)
}

/// The command that stores the configuration each marked center's drawing shows, and a loss
/// for each center the drawing does not determine, at `prefix` + `.atoms[n]`. The derivation is
/// the domain's own, run on a scratch document of the record alone (an import adds a separate
/// component, so a center's neighbourhood is the same there); a center that does not derive
/// keeps its mark and stores nothing, so one degenerate center never refuses the whole import.
/// Also the positions of the atoms whose derived configuration is tetrahedral (a wavy mark
/// derives `unknown`, which no group can hold).
pub(crate) fn derived_configurations(
    commands: &[Command],
    prefix: &str,
    losses: &mut Vec<Loss>,
) -> (Option<Command>, Option<HashSet<usize>>) {
    let id = DocumentId::parse("scratch").expect("scratch identifier is valid");
    let mut scratch = match Document::new(id, "scratch") {
        Ok(scratch) => scratch,
        Err(cause) => return unreadable_record(prefix, &cause.to_string(), losses),
    };
    if let Err(cause) = scratch.apply(commands) {
        return unreadable_record(prefix, &cause.to_string(), losses);
    }
    let instance = scratch.first_instance();
    let drawing = Drawing::of_document(&scratch, instance);
    let centers: std::collections::HashSet<_> = drawing.marked_centers().into_iter().collect();
    let mut readable = Vec::with_capacity(centers.len());
    let mut tetrahedral = HashSet::new();
    for (position, (atom, _)) in scratch.atoms().enumerate() {
        if !centers.contains(&atom) {
            continue;
        }
        match drawing.derive_center(atom) {
            Ok(state) => {
                if is_tetrahedral(&state) {
                    tetrahedral.insert(position);
                }
                readable.push(AtomRef::Created(position));
            }
            Err(cause) => losses.push(unreadable_loss(
                format!("{prefix}.atoms[{}]", position + 1),
                &format!("The drawn marks at this atom do not determine a configuration ({cause}); the marks are kept and no configuration is stored."),
            )),
        }
    }
    let command = (!readable.is_empty()).then_some(Command::SetConfigurationFromDrawing {
        atoms: Some(readable),
        bonds: None,
        instance: None,
    });
    (command, Some(tetrahedral))
}

/// The stored stereo the drawing does not show, as a file written from the document would be
/// read: the atoms whose stored configuration differs from the one their marks and positions
/// derive (or whose derivation fails), then the double bonds likewise, each in ID order.
/// Positions are compared as `written` rounds them, never before: the comparison reads the
/// coordinates the file will hold, not the document's own.
pub(crate) fn stored_not_drawn(
    document: &Document,
    written: impl Fn(Point) -> Point,
) -> (Vec<String>, Vec<String>) {
    let drawing = Drawing::of_document_shown(document, document.first_instance(), written);
    let mut centers = Vec::new();
    let mut double_bonds = Vec::new();
    for (key, state) in document.stereo().iter() {
        match (key, state) {
            (StereoKey::Center(atom), StereoState::Center(stored))
                if drawing.derive_center(atom).ok().as_ref() != Some(stored) =>
            {
                centers.push(atom.to_string());
            }
            (StereoKey::DoubleBond(bond), StereoState::DoubleBond(stored))
                if drawing.derive_double_bond(bond).ok().as_ref() != Some(stored) =>
            {
                double_bonds.push(bond.to_string());
            }
            _ => {}
        }
    }
    (centers, double_bonds)
}

/// The drawing's groups in the order a file lists them (the absolute group, then the `and`
/// groups, then the `or` groups, each kind by interpreted number, an omitted number first), with
/// the members as the file positions `ids` gives their atoms, ascending. `None` when a member is
/// not a mapped atom.
pub(crate) fn listed_groups(
    doc: &DocumentView,
    ids: &HashMap<&str, usize>,
) -> Option<Vec<GroupInput>> {
    let mut groups = Vec::with_capacity(doc.stereo.groups.len());
    for group in &doc.stereo.groups {
        let mut atoms = Vec::with_capacity(group.members.len());
        for member in &group.members {
            atoms.push(*ids.get(member.as_str())?);
        }
        atoms.sort_unstable();
        let (kind, rank) = match group.kind {
            StereoGroupKindDto::Absolute => (GroupKind::Absolute, 0),
            StereoGroupKindDto::And => (GroupKind::And, 1),
            StereoGroupKindDto::Or => (GroupKind::Or, 2),
        };
        groups.push((
            rank,
            group.number,
            GroupInput {
                kind,
                number: group.number,
                atoms,
            },
        ));
    }
    groups.sort_by_key(|(rank, number, _)| (*rank, *number));
    Some(groups.into_iter().map(|(_, _, g)| g).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::DocumentResponse;
    use chemifree_domain::document::{Atom, BondOrder, BondRef, NewAtomPositions};
    use chemifree_domain::element::Element;
    use chemifree_domain::states::AtomKind;
    use chemifree_domain::stereo::{BondEnd, StereoMark};

    /// A position rounded to `step` (a fixed-point grid), as a format with that resolution
    /// would hold it; `step` 0.0001 is the MOL four-decimal rule.
    fn on_grid(step: f64) -> impl Fn(Point) -> Point {
        move |point| {
            let round = |value: f64| (value / step).round() * step;
            Point::new(round(point.x()), round(point.y())).unwrap_or(point)
        }
    }

    /// `count` disjoint carbons with three neighbours at `scale`, one wedge each, every
    /// configuration stored from the drawing; atom `4 * i` is the center of fan `i`. With
    /// `collinear`, the neighbours of every center lie on one line, so no wedge determines it.
    fn fans(scale: f64, count: usize, collinear: bool) -> Vec<Command> {
        let at = |x: f64, y: f64| Command::AddAtom {
            atom: Atom::new(AtomKind::Element(Element::from_symbol("C").unwrap())),
            positions: NewAtomPositions::Single(Point::new(x * scale, y * scale).unwrap()),
            bond_to: None,
        };
        let neighbours = if collinear {
            [(1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]
        } else {
            [(-2.0, -1.0), (2.0, -1.0), (0.0, 2.0)]
        };
        let mut commands = Vec::new();
        for i in 0..count {
            let x = 100.0 * i as f64;
            commands.push(at(x, 0.0));
            commands.extend(neighbours.iter().map(|(dx, dy)| at(x + dx, *dy)));
        }
        for i in 0..count {
            for end in 1..=3 {
                commands.push(Command::AddBond {
                    begin: AtomRef::Created(4 * i),
                    end: AtomRef::Created(4 * i + end),
                    order: BondOrder::Single,
                });
            }
        }
        for i in 0..count {
            commands.push(Command::SetBondMark {
                bond: BondRef::Created(4 * count + 3 * i),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                }),
            });
        }
        commands.push(Command::SetConfigurationFromDrawing {
            atoms: Some((0..count).map(|i| AtomRef::Created(4 * i)).collect()),
            bonds: None,
            instance: None,
        });
        commands
    }

    fn group_of(kind: GroupKind, number: Option<i32>, centers: &[usize]) -> Command {
        Command::CreateStereoGroup {
            kind,
            number,
            atoms: centers.iter().map(|i| AtomRef::Created(4 * i)).collect(),
        }
    }

    fn document_of(commands: &[Command]) -> Document {
        let mut document = Document::new(DocumentId::parse("neutral").unwrap(), "neutral").unwrap();
        document.apply(commands).unwrap();
        document
    }

    fn view_of(document: &Document) -> DocumentView {
        DocumentResponse::new(document, 0, None).document
    }

    #[test]
    fn a_scratch_drawing_that_cannot_be_built_costs_a_loss_not_silence() {
        // Commands that cannot apply: a mark on a bond no command created.
        let commands = [Command::SetBondMark {
            bond: BondRef::Created(5),
            mark: Some(StereoMark::Wavy),
        }];
        let mut losses = Vec::new();
        assert!(
            derived_configurations(&commands, "records[3]", &mut losses)
                .0
                .is_none()
        );
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].feature, "stereo_unreadable");
        assert_eq!(losses[0].source_path, "records[3].atoms");
    }

    #[test]
    fn derived_losses_are_reported_under_the_callers_prefix() {
        // Three neighbours on one line: the wedge does not determine a configuration.
        let mut commands = fans(1.0, 1, true);
        commands.pop(); // the import derives the configuration itself
        let mut losses = Vec::new();
        let (command, tetrahedral) = derived_configurations(&commands, "cdxml.page", &mut losses);
        assert!(command.is_none());
        assert_eq!(tetrahedral, Some(HashSet::new()));
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(losses[0].source_path, "cdxml.page.atoms[1]");
        assert_eq!(losses[0].feature, "stereo_unreadable");
    }

    #[test]
    fn a_free_number_is_the_smallest_unused_positive_one() {
        assert_eq!(free_number(&BTreeSet::new()), Some(1));
        assert_eq!(free_number(&BTreeSet::from([-5, 0, 3])), Some(1));
        assert_eq!(free_number(&BTreeSet::from([1, 2, 4])), Some(3));
        assert_eq!(free_number(&BTreeSet::from([i32::MAX])), Some(1));
        assert_eq!(free_number(&BTreeSet::from([i32::MIN, 1])), Some(2));
    }

    #[test]
    fn a_configuration_that_only_survives_at_four_decimals_needs_consent_on_a_coarse_grid() {
        // The fan is 0.002 points wide: a four-decimal file holds it exactly, a 0.01 grid
        // collapses every neighbour onto the center.
        let document = document_of(&fans(0.001, 1, false));
        let (fine, _) = stored_not_drawn(&document, on_grid(0.0001));
        assert!(fine.is_empty(), "{fine:?}");
        let (coarse, double_bonds) = stored_not_drawn(&document, on_grid(0.01));
        assert_eq!(coarse, vec!["a1".to_owned()]);
        assert!(double_bonds.is_empty());
    }

    #[test]
    fn a_grid_that_keeps_the_picture_asks_for_no_consent() {
        // Ordinary 20-point bonds survive a 10-point grid and not a 100-point one.
        let document = document_of(&fans(10.0, 1, false));
        assert!(stored_not_drawn(&document, on_grid(10.0)).0.is_empty());
        assert_eq!(
            stored_not_drawn(&document, on_grid(100.0)).0,
            vec!["a1".to_owned()]
        );
    }

    #[test]
    fn a_group_member_is_undrawn_only_where_the_grid_loses_its_marks() {
        let mut commands = fans(0.001, 1, false);
        commands.push(group_of(GroupKind::Absolute, None, &[0]));
        let document = document_of(&commands);
        let view = view_of(&document);
        assert!(undrawn_group_members(&document, &view, on_grid(0.0001)).is_empty());
        assert_eq!(
            undrawn_group_members(&document, &view, on_grid(0.01)),
            vec!["a1".to_owned()]
        );
    }

    fn group(kind: GroupKind, number: Option<i32>, atoms: &[usize]) -> GroupInput {
        GroupInput {
            kind,
            number,
            atoms: atoms.to_vec(),
        }
    }

    fn empty_view() -> DocumentView {
        view_of(&Document::new(DocumentId::parse("empty").unwrap(), "empty").unwrap())
    }

    #[test]
    fn group_mapping_names_its_faults_and_leaves_wording_to_the_codec() {
        let view = empty_view();
        let path = |k: usize| format!("g[{k}]");
        let mut losses = Vec::new();
        let all = HashSet::from([0, 1]);
        assert_eq!(
            group_commands(&[], &view, None, path, &mut losses),
            Ok(Vec::new())
        );
        assert_eq!(
            group_commands(
                &[group(GroupKind::And, Some(1), &[0])],
                &view,
                None,
                path,
                &mut losses
            ),
            Err(GroupFault::DrawingUnreadable)
        );
        assert_eq!(
            group_commands(
                &[
                    group(GroupKind::And, Some(1), &[0]),
                    group(GroupKind::Or, Some(1), &[1, 7]),
                ],
                &view,
                Some(&all),
                path,
                &mut losses
            ),
            Err(GroupFault::MemberNotStereocenter { group: 1, atom: 7 })
        );
        assert!(losses.is_empty());
    }

    #[test]
    fn groups_joining_a_document_with_groups_merge_or_renumber_and_say_so() {
        let mut commands = fans(1.0, 3, false);
        commands.push(group_of(GroupKind::Absolute, None, &[0]));
        commands.push(group_of(GroupKind::And, Some(1), &[1]));
        let document = document_of(&commands);
        let view = view_of(&document);
        let all = HashSet::from([0, 1, 2]);
        let mut losses = Vec::new();
        let commands = group_commands(
            &[
                group(GroupKind::Absolute, None, &[0]),
                group(GroupKind::And, Some(1), &[1]),
                group(GroupKind::And, Some(2), &[2]),
            ],
            &view,
            Some(&all),
            |k| format!("g[{k}]"),
            &mut losses,
        )
        .unwrap();
        assert_eq!(commands.len(), 3);
        assert!(matches!(commands[0], Command::AddStereoGroupMembers { .. }));
        // The file's `and 1` is renumbered past 1 and the file's own 2.
        assert!(matches!(
            commands[1],
            Command::CreateStereoGroup {
                number: Some(3),
                ..
            }
        ));
        assert!(matches!(
            commands[2],
            Command::CreateStereoGroup {
                number: Some(2),
                ..
            }
        ));
        let paths: Vec<_> = losses
            .iter()
            .map(|l| (l.feature.as_str(), l.source_path.as_str()))
            .collect();
        assert_eq!(
            paths,
            [
                ("stereo_group_merged", "g[0]"),
                ("stereo_group_number", "g[1]")
            ]
        );
    }

    #[test]
    fn listed_groups_order_by_kind_then_number_with_ascending_members() {
        let mut commands = fans(1.0, 3, false);
        commands.push(group_of(GroupKind::Or, Some(1), &[0]));
        commands.push(group_of(GroupKind::And, Some(2), &[1]));
        commands.push(group_of(GroupKind::Absolute, None, &[2]));
        let view = view_of(&document_of(&commands));
        let ids: HashMap<&str, usize> = view
            .atoms
            .iter()
            .enumerate()
            .map(|(i, atom)| (atom.id.as_str(), i))
            .collect();
        let listed = listed_groups(&view, &ids).unwrap();
        let kinds: Vec<_> = listed.iter().map(|g| (g.kind, g.number)).collect();
        assert_eq!(
            kinds,
            [
                (GroupKind::Absolute, None),
                (GroupKind::And, Some(2)),
                (GroupKind::Or, Some(1)),
            ]
        );
        assert!(listed_groups(&view, &HashMap::new()).is_none());
    }
}
