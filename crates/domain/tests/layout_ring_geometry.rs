//! The geometry of simple rings in the chain layout preview, through the public API. A ring of 3
//! to 12 atoms is a regular polygon of the bond length, a free plain hexagon or pentagon keeps its
//! own template positions, a Kekulé benzene is walked from a1 to a6 and mirrored, an aromatic
//! benzene keeps its positions, and a substituent on a triple-bond member of a ring leaves along
//! the outward bisector. Each expected value is worked out by hand from the rules of
//! `chemifree_domain::layout`.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId,
    NewAtomPositions, Point, RingPattern, RingSize,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{self, Chain, MOVE_TOLERANCE_PT, PlacedComponent, Preview, Scope};
use chemifree_domain::scene::Selection;
use chemifree_domain::settings::Settings;

/// The result of a test or helper, so that a failed setup returns its error instead of panicking.
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The tolerance, in pt or in degrees, of every comparison of a computed geometry.
const GEOMETRY_TOLERANCE: f64 = 1e-9;

/// The bond length of the documents of this file, in pt, which is the default bond length.
const BOND_PT: f64 = 30.0;

fn atom(serial: u32) -> TestResult<AtomId> {
    AtomId::from_serial(serial).ok_or_else(|| format!("atom serial {serial} is not valid").into())
}

fn atoms_of(serials: &[u32]) -> TestResult<Vec<AtomId>> {
    serials.iter().map(|&serial| atom(serial)).collect()
}

fn point(x: f64, y: f64) -> TestResult<Point> {
    Ok(Point::new(x, y)?)
}

fn new_document(id: &str, bond_length_pt: f64) -> TestResult<Document> {
    let id = DocumentId::parse(id).ok_or_else(|| format!("{id} is not a valid document id"))?;
    let settings = Settings {
        bond_length_pt,
        ..Settings::default()
    };
    Ok(Document::new_with_settings(id, "Ring geometry", settings)?)
}

/// A closed ring of carbons 1 to `count` with single bonds and bond length `bond_length_pt`. The
/// carbons sit at arbitrary places, which the layout must ignore. Carbon `n` is command `n`, bonded
/// to the carbon before it, and the last command closes the ring.
fn free_ring(count: u32, bond_length_pt: f64) -> TestResult<Document> {
    let mut document = new_document("free-ring", bond_length_pt)?;
    let mut commands: Vec<Command> = Vec::new();
    for index in 0..count {
        let x = 17.0 * f64::from(index) + 3.0;
        let y = 11.0 * f64::from((index * index) % 7);
        let bond_to = if index == 0 {
            None
        } else {
            Some(BondTo {
                atom: AtomRef::Created(usize::try_from(index - 1)?),
                order: BondOrder::Single,
            })
        };
        commands.push(Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(x, y)?),
            bond_to,
        });
    }
    commands.push(Command::AddBond {
        begin: AtomRef::Created(0),
        end: AtomRef::Created(usize::try_from(count - 1)?),
        order: BondOrder::Single,
    });
    document.apply(&commands)?;
    Ok(document)
}

/// A document with one ring of `size` inserted at the origin, its carbons numbered from a1.
fn template_ring(size: RingSize, pattern: RingPattern) -> TestResult<Document> {
    let mut document = new_document("template-ring", BOND_PT)?;
    let instance = document.first_instance();
    document.apply(&[Command::InsertRing {
        instance,
        center: point(0.0, 0.0)?,
        size,
        pattern,
    }])?;
    Ok(document)
}

/// Prepares the atoms `serials` of `document` with `scope`, and places them.
fn place_serials(document: &Document, serials: &[u32], scope: Scope) -> TestResult<Preview> {
    let atoms = atoms_of(serials)?.into_iter().collect::<BTreeSet<AtomId>>();
    let selection = Selection {
        atoms,
        ..Selection::default()
    };
    let prepared = layout::prepare(document, document.first_instance(), &selection, scope)?;
    Ok(layout::place(&prepared)?)
}

/// The one layout component of `preview`.
fn only_component(preview: &Preview) -> TestResult<&PlacedComponent> {
    match preview.components.as_slice() {
        [component] => Ok(component),
        other => Err(format!("expected one layout component, got {}", other.len()).into()),
    }
}

/// The placed position of every layout atom of `preview`, by atom.
fn placed_positions(preview: &Preview) -> BTreeMap<AtomId, Point> {
    preview
        .positions
        .iter()
        .map(|placement| (placement.atom, placement.after))
        .collect()
}

/// The placed position of atom `serial`.
fn position_of(placed: &BTreeMap<AtomId, Point>, serial: u32) -> TestResult<Point> {
    let id = atom(serial)?;
    placed
        .get(&id)
        .copied()
        .ok_or_else(|| format!("a{serial} has no placed position").into())
}

/// The placed positions of the atoms 1 to `count`, in order.
fn ring_points(placed: &BTreeMap<AtomId, Point>, count: u32) -> TestResult<Vec<Point>> {
    (1..=count)
        .map(|serial| position_of(placed, serial))
        .collect()
}

/// The atom after `serial` around a ring of atoms 1 to `count`.
fn next_around(serial: u32, count: u32) -> u32 {
    if serial == count { 1 } else { serial + 1 }
}

/// The atom before `serial` around a ring of atoms 1 to `count`.
fn previous_around(serial: u32, count: u32) -> u32 {
    if serial == 1 { count } else { serial - 1 }
}

/// The bond between `first` and `second`, in either direction.
fn bond_between(document: &Document, first: AtomId, second: AtomId) -> TestResult<BondId> {
    document
        .bonds()
        .find(|(_, bond)| {
            (bond.begin == first && bond.end == second)
                || (bond.begin == second && bond.end == first)
        })
        .map(|(id, _)| id)
        .ok_or_else(|| "no bond joins the two atoms".into())
}

/// The atoms that `commands` move, in command order.
fn moved_atoms(commands: &[Command]) -> Vec<AtomId> {
    commands
        .iter()
        .flat_map(|command| match command {
            Command::MoveAtoms { atoms, .. } => atoms.clone(),
            _ => Vec::new(),
        })
        .collect()
}

/// The distance in pt between `first` and `second`.
fn distance(first: Point, second: Point) -> f64 {
    (second.x() - first.x()).hypot(second.y() - first.y())
}

/// The angle in degrees at `apex` between the directions to `first` and to `second`, from 0 to 180.
fn interior_angle(apex: Point, first: Point, second: Point) -> f64 {
    let (ux, uy) = (first.x() - apex.x(), first.y() - apex.y());
    let (vx, vy) = (second.x() - apex.x(), second.y() - apex.y());
    (ux * vy - uy * vx)
        .abs()
        .atan2(ux * vx + uy * vy)
        .to_degrees()
}

/// The direction in degrees from `from` to `to`, measured from +x towards +y, which points down.
fn direction(from: Point, to: Point) -> f64 {
    (to.y() - from.y()).atan2(to.x() - from.x()).to_degrees()
}

/// The smaller of the two angles between the directions `first` and `second`, from 0 to 180.
fn angle_gap(first: f64, second: f64) -> f64 {
    let turn = (first - second).rem_euclid(360.0);
    turn.min(360.0 - turn)
}

/// The mean of `points`, which is the centre of a regular polygon through them.
fn centroid(points: &[Point]) -> TestResult<Point> {
    let count = f64::from(u32::try_from(points.len())?);
    let (sum_x, sum_y) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), at| (x + at.x(), y + at.y()));
    point(sum_x / count, sum_y / count)
}

/// Asserts that `actual` lies within `GEOMETRY_TOLERANCE` of (`x`, `y`) on both axes.
fn assert_at(actual: Point, x: f64, y: f64, label: &str) {
    assert!(
        (actual.x() - x).abs() <= GEOMETRY_TOLERANCE
            && (actual.y() - y).abs() <= GEOMETRY_TOLERANCE,
        "{label}: expected ({x}, {y}), got ({}, {})",
        actual.x(),
        actual.y()
    );
}

/// Asserts that the atoms 1 to `count` of `preview` form a regular polygon of `count` sides of
/// `length` pt: every bond is `length`, every interior angle is 180 - 360 / count, and every atom
/// is `length / (2 sin(180 / count))` from the centre of the atoms.
fn assert_regular_polygon(preview: &Preview, count: u32, length: f64) -> TestResult {
    let placed = placed_positions(preview);
    let centre = centroid(&ring_points(&placed, count)?)?;
    let radius = length / (2.0 * (PI / f64::from(count)).sin());
    let angle = 180.0 - 360.0 / f64::from(count);
    for serial in 1..=count {
        let at = position_of(&placed, serial)?;
        let next = next_around(serial, count);
        let previous = previous_around(serial, count);
        let bond = distance(at, position_of(&placed, next)?);
        assert!(
            (bond - length).abs() <= GEOMETRY_TOLERANCE,
            "ring of {count}: bond a{serial}-a{next} is {bond} pt, expected {length}"
        );
        let from_centre = distance(centre, at);
        assert!(
            (from_centre - radius).abs() <= GEOMETRY_TOLERANCE,
            "ring of {count}: a{serial} is {from_centre} pt from the centre, expected {radius}"
        );
        let interior = interior_angle(
            at,
            position_of(&placed, previous)?,
            position_of(&placed, next)?,
        );
        assert!(
            (interior - angle).abs() <= GEOMETRY_TOLERANCE,
            "ring of {count}: the angle at a{serial} is {interior} degrees, expected {angle}"
        );
    }
    Ok(())
}

/// Asserts that the lone ring of atoms 1 to `count` in `preview` is one ring rooted at a1, with no
/// chain, and that every atom is within `tolerance` pt of its before position on both axes.
fn assert_on_template(preview: &Preview, count: u32, tolerance: f64) -> TestResult {
    let component = only_component(preview)?;
    assert_eq!(component.root, atom(1)?, "a free ring is rooted at a1");
    assert!(
        component.chains.is_empty(),
        "a lone ring has no chain, got {:?}",
        component.chains
    );
    let members = atoms_of(&(1..=count).collect::<Vec<u32>>())?;
    assert_eq!(
        component.rings,
        vec![Chain {
            atoms: members,
            branch_from: None,
        }],
        "the ring walks a1 to a2 around the template"
    );
    for placement in &preview.positions {
        let shift_x = (placement.after.x() - placement.before.x()).abs();
        let shift_y = (placement.after.y() - placement.before.y()).abs();
        assert!(
            shift_x <= tolerance && shift_y <= tolerance,
            "{:?} moved from {:?} to {:?}, more than {tolerance} pt",
            placement.atom,
            placement.before,
            placement.after
        );
    }
    Ok(())
}

#[test]
fn every_ring_of_3_to_12_atoms_is_a_regular_polygon_of_side_b() -> TestResult {
    // Each case is a ring size and a bond length. The ring of 7 atoms also runs at 42 pt.
    let cases = (3..=12).map(|count| (count, BOND_PT)).chain([(7, 42.0)]);
    for (count, length) in cases {
        let document = free_ring(count, length)?;
        assert_eq!(
            document.bonds().len(),
            usize::try_from(count)?,
            "a ring of {count} atoms has {count} bonds"
        );
        let serials: Vec<u32> = (1..=count).collect();
        let preview = place_serials(&document, &serials, Scope::Selection)?;
        assert_eq!(
            preview.positions.len(),
            usize::try_from(count)?,
            "every atom of the ring of {count} is placed"
        );
        assert_regular_polygon(&preview, count, length)?;
    }
    Ok(())
}

#[test]
fn a_free_hexagon_and_pentagon_land_on_their_own_template_positions() -> TestResult {
    // The hexagon of InsertRing is already placed exactly. The pentagon is within the tolerance
    // that makes an apply a no-op, since its template is computed with trigonometry.
    for (size, count, tolerance) in [
        (RingSize::Six, 6, 0.0),
        (RingSize::Five, 5, MOVE_TOLERANCE_PT),
    ] {
        let document = template_ring(size, RingPattern::Single)?;
        let preview = place_serials(&document, &[1], Scope::Components)?;
        assert_on_template(&preview, count, tolerance)?;
        let commands = layout::move_commands(&preview, document.first_instance());
        assert!(
            commands.is_empty(),
            "a free ring of {count} on its template gets {} move commands",
            commands.len()
        );
    }
    Ok(())
}

#[test]
fn a_kekule_benzene_is_walked_from_a1_through_a6() -> TestResult {
    let document = template_ring(RingSize::Six, RingPattern::Kekule)?;
    let preview = place_serials(&document, &[1, 2, 3, 4, 5, 6], Scope::Selection)?;
    let component = only_component(&preview)?;
    // Each atom has one single bond, and the walk from a1 starts along its single bond, to a6.
    assert_eq!(
        component.rings,
        vec![Chain {
            atoms: atoms_of(&[1, 6, 5, 4, 3, 2])?,
            branch_from: None,
        }],
        "the Kekulé walk starts a1 to a6"
    );
    // The template with a2 and a6 swapped, and a3 and a5 swapped.
    let placed = placed_positions(&preview);
    let root3 = 15.0 * 3f64.sqrt();
    let expected = [
        (1, 30.0, 0.0),
        (6, 15.0, root3),
        (5, -15.0, root3),
        (4, -30.0, 0.0),
        (3, -15.0, -root3),
        (2, 15.0, -root3),
    ];
    for (serial, x, y) in expected {
        assert_at(position_of(&placed, serial)?, x, y, &format!("a{serial}"));
    }
    let commands = layout::move_commands(&preview, document.first_instance());
    assert_eq!(
        commands.len(),
        4,
        "the mirrored benzene moves four atoms, got {} commands",
        commands.len()
    );
    assert_eq!(
        moved_atoms(&commands),
        atoms_of(&[2, 3, 5, 6])?,
        "the moved atoms are the four that the mirror swaps"
    );
    Ok(())
}

#[test]
fn an_aromatic_benzene_is_laid_out_onto_its_own_positions() -> TestResult {
    let mut document = template_ring(RingSize::Six, RingPattern::Single)?;
    let aromatic: Vec<Command> = document
        .bonds()
        .map(|(bond, _)| Command::UpdateBond {
            bond,
            order: BondOrder::Aromatic,
        })
        .collect();
    document.apply(&aromatic)?;
    assert!(
        document
            .bonds()
            .all(|(_, bond)| bond.order == BondOrder::Aromatic),
        "every bond of the benzene is aromatic"
    );
    let preview = place_serials(&document, &[1, 2, 3, 4, 5, 6], Scope::Selection)?;
    assert_on_template(&preview, 6, MOVE_TOLERANCE_PT)?;
    let commands = layout::move_commands(&preview, document.first_instance());
    assert!(
        commands.is_empty(),
        "an aromatic benzene gets {} move commands",
        commands.len()
    );
    Ok(())
}

#[test]
fn a_substituent_on_a_triple_bond_member_of_cyclooctyne_leaves_along_the_outward_bisector()
-> TestResult {
    // Atom 1 is an end of the triple bond 1-2, and atom 10 is a methyl on it. The layout does not
    // check valence, so the carbon keeps its two ring bonds. A methyl alone would be the free root
    // of the component, since an atom end beats a ring end, and the ring would be entered through
    // it, so no other member would carry it. Atom 9 is a fixed carbon on atom 5, which is not
    // selected. It anchors the ring at atom 5, and the methyl then hangs from atom 1, which is not
    // the entry.
    let mut document = free_ring(8, BOND_PT)?;
    let triple = bond_between(&document, atom(1)?, atom(2)?)?;
    document.apply(&[
        Command::UpdateBond {
            bond: triple,
            order: BondOrder::Triple,
        },
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(-40.0, 0.0)?),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(5)?),
                order: BondOrder::Single,
            }),
        },
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(-20.0, 0.0)?),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(1)?),
                order: BondOrder::Single,
            }),
        },
    ])?;
    let mut serials: Vec<u32> = (1..=8).collect();
    serials.push(10);
    let preview = place_serials(&document, &serials, Scope::Selection)?;
    assert_eq!(
        only_component(&preview)?.root,
        atom(5)?,
        "the ring is anchored at a5, so that it is not a free root"
    );
    assert_eq!(
        preview.positions.len(),
        9,
        "the ring and the methyl are laid out, and the fixed carbon is not"
    );
    assert_regular_polygon(&preview, 8, BOND_PT)?;
    let placed = placed_positions(&preview);
    let centre = centroid(&ring_points(&placed, 8)?)?;
    let member = position_of(&placed, 1)?;
    let methyl = position_of(&placed, 10)?;
    let bond = distance(member, methyl);
    assert!(
        (bond - BOND_PT).abs() <= GEOMETRY_TOLERANCE,
        "the methyl is {bond} pt from a1, expected {BOND_PT}"
    );
    let outward = direction(centre, member);
    let substituent = direction(member, methyl);
    let gap = angle_gap(outward, substituent);
    assert!(
        gap <= GEOMETRY_TOLERANCE,
        "the methyl leaves a1 at {substituent} degrees, not along the outward bisector at \
         {outward} degrees: off by {gap} degrees"
    );
    Ok(())
}
