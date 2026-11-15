//! The simple layout rings of the chain layout preview through the public API: a ring entered from a
//! parent, a ring anchored to a fixed atom, a free ring root, and the units that hang from the
//! members of a ring. Each expected position is worked out by hand from the geometry of the `ring`
//! module. A bond is 30 pt long, so a hexagon has a circumradius of 30 pt, and `r` below is `sqrt(3)`.
//!
//! Every document starts from the hexagon that `InsertRing` draws at the origin: carbons 1 to 6, with
//! carbon 1 at (30, 0). Each further carbon is added with one command and bonded to an earlier carbon,
//! so the serials of atoms and bonds follow the order of creation.

use std::collections::BTreeSet;
use std::error::Error;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId,
    NewAtomPositions, ObjectId, Point, RingPattern, RingSize,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{self, Anchor, Chain, PlacedComponent, Preview, Scope};
use chemifree_domain::scene::Selection;

/// The tolerance, in pt or in degrees, of every comparison of a computed position or angle.
const TOLERANCE: f64 = 1e-9;

/// The result of a test, which can fail with an error of any kind.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn atom(serial: u32) -> TestResult<AtomId> {
    AtomId::from_serial(serial).ok_or_else(|| format!("{serial} is not an atom serial").into())
}

fn bond(serial: u32) -> TestResult<BondId> {
    BondId::from_serial(serial).ok_or_else(|| format!("{serial} is not a bond serial").into())
}

/// The atoms with the given serials, in the order given.
fn atoms(serials: impl IntoIterator<Item = u32>) -> TestResult<Vec<AtomId>> {
    serials.into_iter().map(atom).collect()
}

fn point(x: f64, y: f64) -> TestResult<Point> {
    Ok(Point::new(x, y)?)
}

fn carbon() -> Atom {
    Atom::from(Element::CARBON)
}

/// The command that inserts a hexagon of carbons centred at `centre`, as `InsertRing` draws it.
fn insert_hexagon(instance: ObjectId, centre: (f64, f64)) -> TestResult<Command> {
    Ok(Command::InsertRing {
        instance,
        center: point(centre.0, centre.1)?,
        size: RingSize::Six,
        pattern: RingPattern::Single,
    })
}

/// The hexagon at the origin: carbons 1 to 6, and bonds 1 to 6 with bond 6 closing the ring.
fn hexagon() -> TestResult<Document> {
    let id = DocumentId::parse("ring-units").ok_or("the document identifier is not canonical")?;
    let mut document = Document::new(id, "Ring units")?;
    let instance = document.first_instance();
    document.apply(&[insert_hexagon(instance, (0.0, 0.0))?])?;
    Ok(document)
}

/// Adds a carbon at (x, y), bonded to the carbon with serial `parent`.
fn attach(document: &mut Document, x: f64, y: f64, parent: u32) -> TestResult {
    document.apply(&[Command::AddAtom {
        atom: carbon(),
        positions: NewAtomPositions::Single(point(x, y)?),
        bond_to: Some(BondTo {
            atom: AtomRef::Id(atom(parent)?),
            order: BondOrder::Single,
        }),
    }])?;
    Ok(())
}

/// The hexagon with carbon 7 at (60, 0) bonded to carbon 1: a free methylcyclohexane.
fn methylcyclohexane() -> TestResult<Document> {
    let mut document = hexagon()?;
    attach(&mut document, 60.0, 0.0, 1)?;
    Ok(document)
}

/// Two hexagons, carbons 1 to 6 at the origin and carbons 7 to 12 at (100, 0), joined by the bond
/// a1-a10. Bond 13 is that joining bond.
fn biphenyl() -> TestResult<Document> {
    let mut document = hexagon()?;
    let instance = document.first_instance();
    document.apply(&[insert_hexagon(instance, (100.0, 0.0))?])?;
    document.apply(&[Command::AddBond {
        begin: AtomRef::Id(atom(1)?),
        end: AtomRef::Id(atom(10)?),
        order: BondOrder::Single,
    }])?;
    Ok(document)
}

/// Prepares the atoms with the given serials of `document` as one selection, and places them.
fn preview_of(document: &Document, serials: impl IntoIterator<Item = u32>) -> TestResult<Preview> {
    let selected: BTreeSet<AtomId> = serials.into_iter().map(atom).collect::<TestResult<_>>()?;
    let selection = Selection {
        atoms: selected,
        ..Selection::default()
    };
    let prepared = layout::prepare(
        document,
        document.first_instance(),
        &selection,
        Scope::Selection,
    )?;
    Ok(layout::place(&prepared)?)
}

fn only_component(preview: &Preview) -> TestResult<&PlacedComponent> {
    match preview.components.as_slice() {
        [component] => Ok(component),
        components => Err(format!("expected one component, found {}", components.len()).into()),
    }
}

fn only_ring(component: &PlacedComponent) -> TestResult<&Chain> {
    match component.rings.as_slice() {
        [ring] => Ok(ring),
        rings => Err(format!("expected one ring, found {}", rings.len()).into()),
    }
}

fn two_rings(component: &PlacedComponent) -> TestResult<(&Chain, &Chain)> {
    match component.rings.as_slice() {
        [first, second] => Ok((first, second)),
        rings => Err(format!("expected two rings, found {}", rings.len()).into()),
    }
}

/// The placed position of carbon `serial` in `preview`.
fn after(preview: &Preview, serial: u32) -> TestResult<Point> {
    let id = atom(serial)?;
    preview
        .positions
        .iter()
        .find(|placement| placement.atom == id)
        .map(|placement| placement.after)
        .ok_or_else(|| format!("a{serial} is not laid out").into())
}

fn xy(at: Point) -> (f64, f64) {
    (at.x(), at.y())
}

/// The centre of the placed positions of the carbons `serials`, which are the vertices of one
/// regular polygon.
fn ring_centre(
    preview: &Preview,
    serials: impl IntoIterator<Item = u32>,
) -> TestResult<(f64, f64)> {
    let points: Vec<(f64, f64)> = serials
        .into_iter()
        .map(|serial| after(preview, serial).map(xy))
        .collect::<TestResult<_>>()?;
    let count = f64::from(u32::try_from(points.len())?);
    let (x, y) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), &(px, py)| (x + px, y + py));
    Ok((x / count, y / count))
}

/// `degrees` wrapped into [-180, 180).
fn wrap(degrees: f64) -> f64 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// The direction from `from` to `to`, in degrees.
fn direction(from: (f64, f64), to: (f64, f64)) -> f64 {
    wrap((to.1 - from.1).atan2(to.0 - from.0).to_degrees())
}

fn distance(from: (f64, f64), to: (f64, f64)) -> f64 {
    (to.0 - from.0).hypot(to.1 - from.1)
}

/// The angle, in degrees within [-180, 180), from the outward bisector of `member` to the direction
/// of `unit`, which is bonded to `member`. The bisector of a ring member runs from the centre of
/// the ring `ring` through the member.
fn offset_at(
    preview: &Preview,
    ring: impl IntoIterator<Item = u32>,
    member: u32,
    unit: u32,
) -> TestResult<f64> {
    let centre = ring_centre(preview, ring)?;
    let vertex = xy(after(preview, member)?);
    let bisector = direction(centre, vertex);
    let toward = direction(vertex, xy(after(preview, unit)?));
    Ok(wrap(toward - bisector))
}

/// The distance, in pt, from `at` to the line through `from` and `to`.
fn gap_from_line(from: (f64, f64), to: (f64, f64), at: (f64, f64)) -> f64 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (px, py) = (at.0 - from.0, at.1 - from.1);
    (dx * py - dy * px).abs() / dx.hypot(dy)
}

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < TOLERANCE,
        "{label}: expected {expected}, got {actual}"
    );
}

fn assert_point(actual: (f64, f64), expected: (f64, f64), label: &str) {
    assert!(
        (actual.0 - expected.0).abs() < TOLERANCE && (actual.1 - expected.1).abs() < TOLERANCE,
        "{label}: expected ({}, {}), got ({}, {})",
        expected.0,
        expected.1,
        actual.0,
        actual.1
    );
}

/// Asserts that `at` lies on the ray from `from` through `to`, beyond `from`.
fn assert_on_ray(from: (f64, f64), to: (f64, f64), at: (f64, f64), label: &str) {
    let gap = gap_from_line(from, to, at);
    assert!(gap < TOLERANCE, "{label}: {at:?} is {gap} pt off the line");
    let ahead = (to.0 - from.0) * (at.0 - from.0) + (to.1 - from.1) * (at.1 - from.1);
    assert!(ahead > 0.0, "{label}: {at:?} lies behind {from:?}");
}

#[test]
fn a_free_methylcyclohexane_is_rooted_at_the_methyl() -> TestResult {
    let document = methylcyclohexane()?;
    let preview = preview_of(&document, 1..=7)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(7)?);
    assert_eq!(
        component.chains,
        vec![Chain {
            atoms: atoms([7])?,
            branch_from: None,
        }]
    );
    assert_eq!(
        component.rings,
        vec![Chain {
            atoms: atoms(1..=6)?,
            branch_from: Some(atom(7)?),
        }]
    );
    // Before the shift, carbon 7 is at the origin and the ring is entered at a1 with sign -1, so its
    // centre is (51.96, -30). The walks through a2 and through a6 have equal codes, and the walk
    // through a2 is taken. The before centroid is (60/7, 0), which fixes the shift.
    let r = 3f64.sqrt();
    let shift = (60.0 / 7.0 - 180.0 * r / 7.0, 180.0 / 7.0);
    let before_shift = [
        (7, 0.0, 0.0),
        (1, 15.0 * r, -15.0),
        (2, 30.0 * r, 0.0),
        (3, 45.0 * r, -15.0),
        (4, 45.0 * r, -45.0),
        (5, 30.0 * r, -60.0),
        (6, 15.0 * r, -45.0),
    ];
    for (serial, x, y) in before_shift {
        assert_point(
            xy(after(&preview, serial)?),
            (x + shift.0, y + shift.1),
            &format!("a{serial}"),
        );
    }
    Ok(())
}

#[test]
fn a_ring_anchored_at_a_fixed_atom_hangs_from_it() -> TestResult {
    // Carbon 7 stays at (60, 0), and the selected ring of carbons 1 to 6 hangs from it.
    let document = methylcyclohexane()?;
    let preview = preview_of(&document, 1..=6)?;
    let component = only_component(&preview)?;
    assert_eq!(
        component.anchor,
        Some(Anchor {
            fixed_atom: atom(7)?,
            bond: bond(7)?,
        })
    );
    assert_eq!(component.root, atom(1)?);
    assert!(component.chains.is_empty());
    assert_eq!(
        component.rings,
        vec![Chain {
            atoms: atoms(1..=6)?,
            branch_from: Some(atom(7)?),
        }]
    );
    // The ring is entered at a1 with direction -30 and sign -1, and a1 is 30 pt from carbon 7. The
    // walk through a2 is taken, as in a free ring.
    let r = 3f64.sqrt();
    let expected = [
        (1, 15.0 * r, -15.0),
        (2, 30.0 * r, 0.0),
        (3, 45.0 * r, -15.0),
        (4, 45.0 * r, -45.0),
        (5, 30.0 * r, -60.0),
        (6, 15.0 * r, -45.0),
    ];
    for (serial, x, y) in expected {
        assert_point(
            xy(after(&preview, serial)?),
            (60.0 + x, y),
            &format!("a{serial}"),
        );
    }
    Ok(())
}

#[test]
fn two_substituents_on_one_ring_member_take_the_bisector_plus_and_minus_40_degrees() -> TestResult {
    // Carbons 7 and 8 form an ethyl on carbon 1, and carbons 9 and 10 are methyls on carbon 4.
    let r = 3f64.sqrt();
    let mut document = hexagon()?;
    attach(&mut document, 60.0, 0.0, 1)?;
    attach(&mut document, 90.0, 0.0, 7)?;
    attach(&mut document, -60.0, 0.0, 4)?;
    attach(&mut document, -45.0, -15.0 * r, 4)?;
    let preview = preview_of(&document, 1..=10)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(8)?);
    // The two units at carbon 4 take the slots at 40 degrees on either side of its bisector. Their
    // angle is 80 degrees.
    let mut offsets = [
        offset_at(&preview, 1..=6, 4, 9)?,
        offset_at(&preview, 1..=6, 4, 10)?,
    ];
    offsets.sort_by(f64::total_cmp);
    let [low, high] = offsets;
    assert_close(low, -40.0, "carbon 9 from the bisector at carbon 4");
    assert_close(high, 40.0, "carbon 10 from the bisector at carbon 4");
    assert_close(high - low, 80.0, "the angle 9-4-10");
    Ok(())
}

#[test]
fn an_entry_atom_with_two_exocyclic_units_puts_its_parent_on_the_first_slot() -> TestResult {
    // Carbons 7 and 8 are both methyls on carbon 1, so the ring is entered from carbon 7.
    let r = 3f64.sqrt();
    let mut document = hexagon()?;
    attach(&mut document, 60.0, 0.0, 1)?;
    attach(&mut document, 45.0, 15.0 * r, 1)?;
    let preview = preview_of(&document, 1..=8)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(7)?);
    assert_eq!(only_ring(component)?.atoms.first(), Some(&atom(1)?));
    // The sign is -1, so the parent, carbon 7, takes the slot at +40 degrees on the -s side, and
    // carbon 8 takes -40 degrees.
    assert_close(
        offset_at(&preview, 1..=6, 1, 7)?,
        40.0,
        "the parent carbon 7 from the bisector at carbon 1",
    );
    assert_close(
        offset_at(&preview, 1..=6, 1, 8)?,
        -40.0,
        "carbon 8 from the bisector at carbon 1",
    );
    Ok(())
}

#[test]
fn a_free_biphenyl_is_rooted_at_a_ring_and_enters_the_second_by_r8() -> TestResult {
    let document = biphenyl()?;
    let preview = preview_of(&document, 1..=12)?;
    let component = only_component(&preview)?;
    assert!(component.chains.is_empty());
    let (first, second) = two_rings(component)?;
    // Both rings have equal codes, so the root is the ring whose entry has the smaller identifier.
    // The walk from a2 that ends at a1 is the smallest walk, because a1, the bearer of the second
    // ring, comes last.
    assert_eq!(first.atoms, atoms([2, 3, 4, 5, 6, 1])?);
    assert_eq!(first.branch_from, None);
    assert_eq!(first.atoms.first(), Some(&component.root));
    assert_eq!(second.atoms.first(), Some(&atom(10)?));
    assert_eq!(second.branch_from, Some(atom(1)?));
    // The second ring is entered from a1 along the bond a1-a10, which lies at 300 degrees.
    let (a1, a10) = (xy(after(&preview, 1)?), xy(after(&preview, 10)?));
    assert_close(distance(a1, a10), 30.0, "the bond a1-a10");
    let centre_b = ring_centre(&preview, 7..=12)?;
    assert_on_ray(a1, a10, centre_b, "the centre of the second ring");
    Ok(())
}

#[test]
fn a_ring_child_of_a_ring_member_is_entered_along_that_members_bisector() -> TestResult {
    // Carbon 13 is a methyl on carbon 4 of the first ring of a biphenyl.
    let mut document = biphenyl()?;
    attach(&mut document, -60.0, 0.0, 4)?;
    let preview = preview_of(&document, 1..=13)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(13)?);
    assert_eq!(
        component.chains,
        vec![Chain {
            atoms: atoms([13])?,
            branch_from: None,
        }]
    );
    let (first, second) = two_rings(component)?;
    assert_eq!(first.atoms, atoms([4, 3, 2, 1, 6, 5])?);
    assert_eq!(first.branch_from, Some(atom(13)?));
    assert_eq!(second.atoms.first(), Some(&atom(10)?));
    assert_eq!(second.branch_from, Some(atom(1)?));
    // The centres of both rings and carbons 1 and 10 lie on one line, the bisector of carbon 1.
    let centre_a = ring_centre(&preview, 1..=6)?;
    let centre_b = ring_centre(&preview, 7..=12)?;
    let (a1, a10) = (xy(after(&preview, 1)?), xy(after(&preview, 10)?));
    assert_on_ray(
        centre_a,
        a1,
        a10,
        "the bond from carbon 1 to the second ring",
    );
    assert_on_ray(centre_a, a1, centre_b, "the centre of the second ring");
    Ok(())
}

#[test]
fn a_chain_ring_chain_component_lists_each_atom_in_one_run() -> TestResult {
    // A chain of carbons 8 and 7 leads to the hexagon at carbon 1, and a chain of carbons 9 and 10
    // leads from carbon 4.
    let mut document = hexagon()?;
    attach(&mut document, 60.0, 0.0, 1)?;
    attach(&mut document, 90.0, 0.0, 7)?;
    attach(&mut document, -60.0, 0.0, 4)?;
    attach(&mut document, -90.0, 0.0, 9)?;
    let preview = preview_of(&document, 1..=10)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(8)?);
    assert_eq!(
        component.chains,
        vec![
            Chain {
                atoms: atoms([8, 7])?,
                branch_from: None,
            },
            Chain {
                atoms: atoms([9, 10])?,
                branch_from: Some(atom(4)?),
            },
        ]
    );
    assert_eq!(
        component.rings,
        vec![Chain {
            atoms: atoms(1..=6)?,
            branch_from: Some(atom(7)?),
        }]
    );
    // Every layout atom is in exactly one run, and no run is empty.
    assert!(
        component
            .chains
            .iter()
            .chain(&component.rings)
            .all(|run| !run.atoms.is_empty())
    );
    let mut placed: Vec<AtomId> = component
        .chains
        .iter()
        .chain(&component.rings)
        .flat_map(|run| run.atoms.iter().copied())
        .collect();
    placed.sort();
    assert_eq!(placed, atoms(1..=10)?);
    Ok(())
}

#[test]
fn a_substituted_ring_neighbour_comes_first_in_the_walk() -> TestResult {
    // Carbon 7 is on carbon 1, and carbon 8 is on carbon 6, so the two substituents are ortho.
    let mut document = hexagon()?;
    attach(&mut document, 60.0, 0.0, 1)?;
    attach(&mut document, 30.0, -51.96, 6)?;
    let preview = preview_of(&document, 1..=8)?;
    let component = only_component(&preview)?;
    assert_eq!(component.root, atom(7)?);
    // The walk through a6, which carries a substituent, is smaller than the walk through a2, so
    // a6 is the first neighbour of the entry.
    assert_eq!(only_ring(component)?.atoms, atoms([1, 6, 5, 4, 3, 2])?);
    Ok(())
}
