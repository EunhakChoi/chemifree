//! The comparison of stored and drawn stereo: which items are listed, the five agreements, the
//! unknown-against-unknown rule, the non-atom label, the weak-evidence strength, the
//! per-instance entries and the findings that do not depend on an instance. Expected values are
//! written by hand; E1 is the hand-computed drawing of `stereo_drawing.rs` (F top, Cl lower
//! right with a wedge whose narrow end is the center, Br lower left: clockwise over
//! `[F, Cl, Br, H]`, `|D| = 3.4647`; moving Br to the upper right gives anticlockwise).

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    NewAtomPositions, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoError, StereoMark, Turn,
};
use chemifree_domain::stereo_drawing::{
    DEGENERACY_LIMIT, DeriveError, NotDerivable, WEAK_EVIDENCE,
};
use chemifree_domain::stereo_inspection::{
    Agreement, InspectError, Inspection, Plan, Underivable, centers_agree, inspect, label_differs,
};

fn aid(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bid(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn slot(serial: u32) -> Slot {
    Slot::Atom(aid(serial))
}

fn tetra(slots: [Slot; 4], turn: Turn) -> CenterState {
    CenterState::tetrahedral(slots, turn).unwrap()
}

fn unknown(slots: [Slot; 4]) -> CenterState {
    CenterState::unknown(slots).unwrap()
}

fn halo_slots(fourth: Slot) -> [Slot; 4] {
    [slot(2), slot(3), slot(4), fourth]
}

/// Builds a document by commands: atoms `a1...` and bonds `b1...` in the order they are added.
#[derive(Default)]
struct Build {
    commands: Vec<Command>,
    atoms: u32,
    bonds: u32,
}

impl Build {
    fn atom(&mut self, symbol: &str, x: f64, y: f64) -> u32 {
        self.atom_as(Atom::from(Element::from_symbol(symbol).unwrap()), x, y)
    }

    fn atom_as(&mut self, atom: Atom, x: f64, y: f64) -> u32 {
        self.commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(x, y).unwrap()),
            bond_to: None,
        });
        self.atoms += 1;
        self.atoms
    }

    fn bond(&mut self, begin: u32, end: u32, order: BondOrder) -> u32 {
        self.commands.push(Command::AddBond {
            begin: AtomRef::Id(aid(begin)),
            end: AtomRef::Id(aid(end)),
            order,
        });
        self.bonds += 1;
        self.bonds
    }

    fn mark(&mut self, bond: u32, mark: StereoMark) {
        self.commands.push(Command::SetBondMark {
            bond: BondRef::Id(bid(bond)),
            mark: Some(mark),
        });
    }

    fn state(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// Carbon `a1` at (x, y) with F `a2` above, Cl `a3` lower right and Br `a4` lower left (E1),
    /// bonds `b1`, `b2`, `b3` from the carbon.
    fn halo(&mut self, x: f64, y: f64) {
        let center = self.atom("C", x, y);
        let f = self.atom("F", x, y - 30.0);
        let cl = self.atom("Cl", x + 26.0, y + 15.0);
        let br = self.atom("Br", x - 26.0, y + 15.0);
        for end in [f, cl, br] {
            self.bond(center, end, BondOrder::Single);
        }
    }

    fn document(&self) -> Document {
        let mut document =
            Document::new(DocumentId::parse("doc-inspect").unwrap(), "Inspect").unwrap();
        document.apply(&self.commands).unwrap();
        document
    }
}

fn wedge_begin() -> StereoMark {
    StereoMark::Wedge {
        narrow: BondEnd::Begin,
    }
}

fn store_center(center: u32, slots: [Slot; 4], turn: Option<Turn>) -> Command {
    let neighbors = slots.map(|slot| match slot {
        Slot::Atom(id) => SlotRef::Atom(AtomRef::Id(id)),
        Slot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
        Slot::LonePair => SlotRef::LonePair,
    });
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(aid(center)),
        configuration: Some(match turn {
            Some(turn) => AtomConfiguration::Tetrahedral { neighbors, turn },
            None => AtomConfiguration::Unknown { neighbors },
        }),
    }
}

/// The E1 halo with the wedge on the Cl bond, and `stored` stated for its center.
fn stored_halo(stored: Option<Turn>, fourth: Slot) -> Document {
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    build.mark(2, wedge_begin());
    if let Some(turn) = stored {
        build.state(store_center(1, halo_slots(fourth), Some(turn)));
    }
    build.document()
}

fn all(document: &Document) -> Inspection {
    let instances: Vec<ObjectId> = document.instances().collect();
    let plan = Plan::of(document).unwrap();
    inspect(document, &plan, &instances).unwrap()
}

fn move_br(document: &mut Document, instance: Option<ObjectId>) {
    // Br from the lower left to the upper right: E3, which reverses the turn.
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![aid(4)],
            delta: Vector {
                dx: 52.0,
                dy: -30.0,
            },
            instance,
        }])
        .unwrap();
}

#[test]
fn the_plan_lists_stored_marked_and_crossed_items_but_not_plain_ones_ascending() {
    let mut build = Build::default();
    build.halo(100.0, 100.0); // a1..a4, marked below
    build.mark(2, wedge_begin());
    build.halo(300.0, 100.0); // a5..a8: unmarked, unstored: not an item
    build.halo(500.0, 100.0); // a9..a12: unmarked but stored: an item
    build.state(store_center(
        9,
        [slot(10), slot(11), slot(12), Slot::ImplicitHydrogen],
        Some(Turn::Clockwise),
    ));
    // A terminal wedge bears on nothing; an unmarked, unstored double bond; a crossed one; a
    // stored one.
    let ends: Vec<u32> = (0..6)
        .map(|n| build.atom("C", 30.0 * f64::from(n), 300.0))
        .collect();
    let plain = build.bond(ends[0], ends[1], BondOrder::Double);
    let crossed = build.bond(ends[2], ends[3], BondOrder::Double);
    let stored = build.bond(ends[4], ends[5], BondOrder::Double);
    build.mark(crossed, StereoMark::Crossed);
    build.state(Command::SetBondConfiguration {
        bond: BondRef::Id(bid(stored)),
        configuration: Some(BondConfiguration::Unknown),
    });
    let document = build.document();

    let plan = Plan::of(&document).unwrap();
    assert_eq!(plan.atoms(), [aid(1), aid(9)]);
    assert_eq!(plan.bonds(), [bid(crossed), bid(stored)]);
    assert!(!plan.bonds().contains(&bid(plain)));
    assert_eq!(plan.len(), 4);
    // Marked is a fact of the entry, not of a reason: the stored-only center is unmarked.
    assert!(plan.is_marked(aid(1)) && !plan.is_marked(aid(9)));
    let inspection = all(&document);
    assert_eq!(
        inspection
            .atoms
            .iter()
            .map(|entry| (entry.atom, entry.marked))
            .collect::<Vec<_>>(),
        [(aid(1), true), (aid(9), false)]
    );
}

#[test]
fn a_stored_turn_that_matches_the_drawing_agrees_with_its_strength() {
    let document = stored_halo(Some(Turn::Clockwise), Slot::ImplicitHydrogen);
    let inspection = all(&document);
    let [entry] = &inspection.atoms[..] else {
        panic!("one center expected: {inspection:?}");
    };
    assert_eq!(entry.atom, aid(1));
    assert_eq!(
        entry.stored,
        Some(tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise))
    );
    let [drawn] = &entry.drawn[..] else {
        panic!("one instance expected");
    };
    assert_eq!(
        drawn.drawn,
        Ok(tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise))
    );
    assert_eq!(drawn.agreement, Agreement::Agrees);
    assert!(!drawn.label_differs);
    let strength = drawn.strength.expect("a tetrahedral result has a strength");
    assert!(
        (strength - 3.4647).abs() < 1e-3,
        "|D| of E1 is 3.4647, got {strength}"
    );
    assert!(!drawn.is_weak_evidence());
}

#[test]
fn moving_an_atom_across_the_center_disagrees_and_changes_nothing_stored() {
    let mut document = stored_halo(Some(Turn::Clockwise), Slot::ImplicitHydrogen);
    move_br(&mut document, None);
    let before = document.clone();
    let inspection = all(&document);
    assert_eq!(document, before, "an inspection reads only");
    let drawn = &inspection.atoms[0].drawn[0];
    assert_eq!(
        drawn.drawn,
        Ok(tetra(
            halo_slots(Slot::ImplicitHydrogen),
            Turn::Anticlockwise
        )),
        "E3: the hand-computed anticlockwise"
    );
    assert_eq!(drawn.agreement, Agreement::Disagrees);
    assert_eq!(
        inspection.atoms[0].stored,
        Some(tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise)),
        "the stored record is reported as it is"
    );
}

#[test]
fn a_stored_center_no_mark_bears_on_is_only_stored_with_the_no_marks_reason() {
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    build.state(store_center(
        1,
        halo_slots(Slot::ImplicitHydrogen),
        Some(Turn::Clockwise),
    ));
    let inspection = all(&build.document());
    let drawn = &inspection.atoms[0].drawn[0];
    assert_eq!(drawn.agreement, Agreement::OnlyStored);
    assert_eq!(
        drawn.drawn,
        Err(Underivable::NotDerivable(NotDerivable::NoMarks))
    );
    assert!(drawn.drawn.unwrap_err().is_no_marks());
    assert_eq!(drawn.strength, None);
}

#[test]
fn a_marked_center_with_nothing_stored_is_only_drawn() {
    let inspection = all(&stored_halo(None, Slot::ImplicitHydrogen));
    let entry = &inspection.atoms[0];
    assert_eq!(entry.stored, None);
    assert_eq!(entry.drawn[0].agreement, Agreement::OnlyDrawn);
    assert_eq!(
        entry.drawn[0].drawn,
        Ok(tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise))
    );
}

#[test]
fn a_marked_center_the_drawing_cannot_read_and_nothing_stored_is_neither() {
    // A wedge at the center and a wavy bond to a terminal atom: conflicting marks.
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    build.mark(2, wedge_begin());
    build.mark(1, StereoMark::Wavy);
    let inspection = all(&build.document());
    let drawn = &inspection.atoms[0].drawn[0];
    assert_eq!(drawn.agreement, Agreement::Neither);
    assert_eq!(
        drawn.drawn,
        Err(Underivable::NotDerivable(NotDerivable::ConflictingMarks))
    );
    assert_eq!(
        drawn.drawn.unwrap_err().code(),
        "stereo_not_derivable_conflicting_marks"
    );
}

#[test]
fn unknown_agrees_with_unknown_and_disagrees_with_a_definite_turn_either_way() {
    // The wavy bond alone: the drawing says unknown.
    let wavy = |stored: Option<Turn>| {
        let mut build = Build::default();
        build.halo(100.0, 100.0);
        build.mark(1, StereoMark::Wavy);
        build.state(store_center(1, halo_slots(Slot::ImplicitHydrogen), stored));
        all(&build.document())
    };
    let both_unknown = wavy(None);
    let drawn = &both_unknown.atoms[0].drawn[0];
    assert_eq!(drawn.drawn, Ok(unknown(halo_slots(Slot::ImplicitHydrogen))));
    assert_eq!(drawn.agreement, Agreement::Agrees);
    assert_eq!(drawn.strength, None, "an unknown result reads no geometry");

    let definite_against_unknown = wavy(Some(Turn::Clockwise));
    assert_eq!(
        definite_against_unknown.atoms[0].drawn[0].agreement,
        Agreement::Disagrees
    );

    // Stored unknown against a drawn definite turn.
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    build.mark(2, wedge_begin());
    build.state(store_center(1, halo_slots(Slot::ImplicitHydrogen), None));
    let unknown_against_definite = all(&build.document());
    assert_eq!(
        unknown_against_definite.atoms[0].drawn[0].agreement,
        Agreement::Disagrees
    );
}

#[test]
fn centers_agree_compares_geometry_with_the_non_atom_position_unlabelled() {
    let hydrogen = tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise);
    let lone_pair = tetra(halo_slots(Slot::LonePair), Turn::Clockwise);
    let opposite = tetra(halo_slots(Slot::LonePair), Turn::Anticlockwise);
    assert!(centers_agree(&hydrogen, &lone_pair), "the label is ignored");
    assert!(!centers_agree(&hydrogen, &opposite));
    assert!(centers_agree(
        &unknown(halo_slots(Slot::ImplicitHydrogen)),
        &unknown(halo_slots(Slot::LonePair))
    ));
    assert!(!centers_agree(
        &hydrogen,
        &unknown(halo_slots(Slot::ImplicitHydrogen))
    ));
    assert!(!centers_agree(
        &unknown(halo_slots(Slot::ImplicitHydrogen)),
        &hydrogen
    ));
    // Another neighbor set is another center.
    let other = tetra(
        [slot(2), slot(3), slot(5), Slot::ImplicitHydrogen],
        Turn::Clockwise,
    );
    assert!(!centers_agree(&hydrogen, &other));
    // Four atoms and no non-atom position: nothing to label.
    let four = tetra([slot(2), slot(3), slot(4), slot(5)], Turn::Clockwise);
    assert!(centers_agree(&four, &four));
    assert!(!label_differs(&four, &four));
}

#[test]
fn label_differs_only_when_both_non_atom_positions_exist_and_differ() {
    let hydrogen = tetra(halo_slots(Slot::ImplicitHydrogen), Turn::Clockwise);
    let lone_pair = tetra(halo_slots(Slot::LonePair), Turn::Clockwise);
    assert!(label_differs(&hydrogen, &lone_pair));
    assert!(label_differs(
        &unknown(halo_slots(Slot::LonePair)),
        &hydrogen
    ));
    assert!(!label_differs(&hydrogen, &hydrogen));
    let four = tetra([slot(2), slot(3), slot(4), slot(5)], Turn::Clockwise);
    assert!(!label_differs(&four, &hydrogen));
}

/// S(=O)(C)C drawn as E1 (O on top, wedge on the first carbon): the sulfur has no hydrogen, so
/// its fourth position is a lone pair.
fn sulfoxide(stored: Option<(Slot, Turn)>) -> Document {
    let mut build = Build::default();
    let sulfur = build.atom("S", 100.0, 100.0);
    let oxygen = build.atom("O", 100.0, 70.0);
    let first = build.atom("C", 126.0, 115.0);
    let second = build.atom("C", 74.0, 115.0);
    build.bond(sulfur, oxygen, BondOrder::Double);
    let wedged = build.bond(sulfur, first, BondOrder::Single);
    build.bond(sulfur, second, BondOrder::Single);
    build.mark(wedged, wedge_begin());
    if let Some((fourth, turn)) = stored {
        build.state(store_center(1, halo_slots(fourth), Some(turn)));
    }
    build.document()
}

#[test]
fn a_stored_hydrogen_against_a_drawn_lone_pair_agrees_and_reports_the_label() {
    let drawn_lone_pair = tetra(halo_slots(Slot::LonePair), Turn::Clockwise);
    let same_turn = all(&sulfoxide(Some((Slot::ImplicitHydrogen, Turn::Clockwise))));
    let drawn = &same_turn.atoms[0].drawn[0];
    assert_eq!(drawn.drawn, Ok(drawn_lone_pair));
    assert_eq!(
        drawn.agreement,
        Agreement::Agrees,
        "the label is not compared"
    );
    assert!(drawn.label_differs);

    let other_turn = all(&sulfoxide(Some((
        Slot::ImplicitHydrogen,
        Turn::Anticlockwise,
    ))));
    let drawn = &other_turn.atoms[0].drawn[0];
    assert_eq!(drawn.agreement, Agreement::Disagrees);
    assert!(
        drawn.label_differs,
        "the label difference does not depend on the agreement"
    );

    let same_label = all(&sulfoxide(Some((Slot::LonePair, Turn::Clockwise))));
    assert!(!same_label.atoms[0].drawn[0].label_differs);
}

/// A straight pair of plain bonds (F up, Cl down but tilted by `tilt` radians) and a wedge to Br.
fn nearly_flat(tilt: f64) -> Document {
    let mut build = Build::default();
    let center = build.atom("C", 100.0, 100.0);
    let up = build.atom("F", 100.0, 70.0);
    let down = build.atom("Cl", 100.0 + 30.0 * tilt.sin(), 100.0 + 30.0 * tilt.cos());
    let wedged = build.atom("Br", 126.0, 115.0);
    build.bond(center, up, BondOrder::Single);
    build.bond(center, down, BondOrder::Single);
    let bond = build.bond(center, wedged, BondOrder::Single);
    build.mark(bond, wedge_begin());
    build.document()
}

#[test]
fn a_nearly_flat_drawing_is_derived_with_weak_evidence_below_the_named_limit() {
    // `|D|` is about 4 * tilt for this drawing (hand check: 0.05 for 0.0125 rad, 0.2 for 0.05).
    let weak = all(&nearly_flat(0.0125));
    let drawn = &weak.atoms[0].drawn[0];
    assert!(drawn.drawn.is_ok(), "derived, not refused: {drawn:?}");
    let strength = drawn.strength.unwrap();
    assert!(
        (DEGENERACY_LIMIT..WEAK_EVIDENCE).contains(&strength),
        "weak: {strength}"
    );
    assert!(drawn.is_weak_evidence());

    let firm = all(&nearly_flat(0.05));
    let drawn = &firm.atoms[0].drawn[0];
    assert!(drawn.strength.unwrap() >= WEAK_EVIDENCE);
    assert!(!drawn.is_weak_evidence());

    // Below the degeneracy limit nothing is derived.
    let flat = all(&nearly_flat(0.0001));
    assert_eq!(
        flat.atoms[0].drawn[0].drawn,
        Err(Underivable::NotDerivable(NotDerivable::Degenerate))
    );
    assert!(!flat.atoms[0].drawn[0].is_weak_evidence());
}

fn two_instance_halo() -> Document {
    let mut document = stored_halo(Some(Turn::Clockwise), Slot::ImplicitHydrogen);
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 200.0, dy: 0.0 },
        }])
        .unwrap();
    let first = document.first_instance();
    let second = document.instances().find(|&id| id != first).unwrap();
    move_br(&mut document, Some(second));
    document
}

#[test]
fn each_inspected_instance_has_its_own_drawn_entry_in_instance_order() {
    let document = two_instance_halo();
    let instances: Vec<ObjectId> = document.instances().collect();
    assert_eq!(instances.len(), 2);
    let inspection = all(&document);
    let drawn = &inspection.atoms[0].drawn;
    assert_eq!(
        drawn.iter().map(|d| d.instance).collect::<Vec<_>>(),
        instances
    );
    assert_eq!(
        drawn[0].agreement,
        Agreement::Agrees,
        "the first instance is unmoved"
    );
    assert_eq!(
        drawn[1].agreement,
        Agreement::Disagrees,
        "Br moved in the second only"
    );

    // Naming one instance gives one entry.
    let plan = Plan::of(&document).unwrap();
    let only_second = inspect(&document, &plan, &instances[1..]).unwrap();
    assert_eq!(only_second.atoms[0].drawn.len(), 1);
    assert_eq!(only_second.atoms[0].drawn[0].instance, instances[1]);
    assert_eq!(
        only_second.atoms[0].drawn[0].agreement,
        Agreement::Disagrees
    );
}

/// Zig-zag a1 (70,115), a2 (100,100) = a3 (130,100), a4 (160,85), double bond b2: trans.
fn alkene(extra: impl FnOnce(&mut Build)) -> Document {
    let mut build = Build::default();
    let a1 = build.atom("C", 70.0, 115.0);
    let a2 = build.atom("C", 100.0, 100.0);
    let a3 = build.atom("C", 130.0, 100.0);
    let a4 = build.atom("C", 160.0, 85.0);
    build.bond(a1, a2, BondOrder::Single);
    build.bond(a2, a3, BondOrder::Double);
    build.bond(a3, a4, BondOrder::Single);
    extra(&mut build);
    build.document()
}

fn store_relation(relation: Relation) -> Command {
    Command::SetBondConfiguration {
        bond: BondRef::Id(bid(2)),
        configuration: Some(BondConfiguration::Relative {
            begin_neighbor: SlotRef::Atom(AtomRef::Id(aid(1))),
            end_neighbor: SlotRef::Atom(AtomRef::Id(aid(4))),
            relation,
        }),
    }
}

fn trans() -> DoubleBondState {
    DoubleBondState::Relative {
        begin_ref: slot(1),
        end_ref: slot(4),
        relation: Relation::OppositeSides,
    }
}

#[test]
fn a_stored_double_bond_agrees_or_disagrees_with_its_geometry() {
    let agrees = all(&alkene(|build| {
        build.state(store_relation(Relation::OppositeSides))
    }));
    let drawn = &agrees.bonds[0].drawn[0];
    assert_eq!(drawn.drawn, Ok(trans()), "E6: the zig-zag is trans");
    assert_eq!(drawn.agreement, Agreement::Agrees);
    assert!(drawn.same_side_ends.is_empty());

    let disagrees = all(&alkene(|build| {
        build.state(store_relation(Relation::SameSide))
    }));
    assert_eq!(disagrees.bonds[0].drawn[0].agreement, Agreement::Disagrees);
    assert_eq!(
        disagrees.bonds[0].stored,
        Some(DoubleBondState::Relative {
            begin_ref: slot(1),
            end_ref: slot(4),
            relation: Relation::SameSide
        })
    );
}

#[test]
fn a_crossed_double_bond_is_drawn_unknown_and_compared_like_a_center() {
    let crossed = |build: &mut Build| build.mark(2, StereoMark::Crossed);
    let only_drawn = all(&alkene(crossed));
    assert_eq!(only_drawn.bonds[0].stored, None);
    assert_eq!(
        only_drawn.bonds[0].drawn[0].drawn,
        Ok(DoubleBondState::Unknown)
    );
    assert_eq!(only_drawn.bonds[0].drawn[0].agreement, Agreement::OnlyDrawn);

    let both_unknown = all(&alkene(|build| {
        crossed(build);
        build.state(Command::SetBondConfiguration {
            bond: BondRef::Id(bid(2)),
            configuration: Some(BondConfiguration::Unknown),
        });
    }));
    assert_eq!(both_unknown.bonds[0].drawn[0].agreement, Agreement::Agrees);

    let definite_against_unknown = all(&alkene(|build| {
        crossed(build);
        build.state(store_relation(Relation::OppositeSides));
    }));
    assert_eq!(
        definite_against_unknown.bonds[0].drawn[0].agreement,
        Agreement::Disagrees
    );
}

#[test]
fn a_crossed_bond_the_model_cannot_hold_is_neither_with_the_structural_reason() {
    // C=C=C: the crossed bond has a cumulated double bond at an end.
    let mut build = Build::default();
    let a1 = build.atom("C", 0.0, 0.0);
    let a2 = build.atom("C", 30.0, 0.0);
    let a3 = build.atom("C", 60.0, 0.0);
    build.bond(a1, a2, BondOrder::Double);
    let crossed = build.bond(a2, a3, BondOrder::Double);
    build.mark(crossed, StereoMark::Crossed);
    let inspection = all(&build.document());
    let drawn = &inspection.bonds[0].drawn[0];
    assert_eq!(drawn.agreement, Agreement::Neither);
    assert_eq!(
        drawn.drawn,
        Err(Underivable::Structure(StereoError::GeometryUnsupported))
    );
    assert_eq!(
        drawn.drawn.unwrap_err().code(),
        "stereo_geometry_unsupported"
    );
}

#[test]
fn an_end_with_two_substituents_on_one_side_of_the_double_bond_is_listed() {
    // a5 below the axis beside a1 (also below): both substituents of a2 on the same side.
    let document = alkene(|build| {
        let extra = build.atom("C", 80.0, 130.0);
        build.bond(2, extra, BondOrder::Single);
        build.state(store_relation(Relation::OppositeSides));
    });
    let inspection = all(&document);
    let drawn = &inspection.bonds[0].drawn[0];
    assert_eq!(drawn.same_side_ends, [aid(2)]);
    assert!(
        drawn.drawn.is_ok(),
        "the lowest-identifier neighbor still decides"
    );

    // Substituents on both sides are not listed.
    let split = alkene(|build| {
        let extra = build.atom("C", 80.0, 70.0);
        build.bond(2, extra, BondOrder::Single);
        build.state(store_relation(Relation::OppositeSides));
    });
    assert!(all(&split).bonds[0].drawn[0].same_side_ends.is_empty());
}

#[test]
fn marks_that_cannot_bear_and_wavy_bonds_between_centers_are_found_once_per_bond() {
    let mut build = Build::default();
    build.halo(100.0, 100.0); // a1 center, a2 F, a3 Cl, a4 Br; b1 C-F, b2 C-Cl, b3 C-Br
    // A hash drawn from the F end: F is terminal, the other end is the center.
    build.commands.push(Command::SetBondMark {
        bond: BondRef::Id(bid(1)),
        mark: Some(StereoMark::Hash {
            narrow: BondEnd::End,
        }),
    });
    // A hash on the Br bond with its narrow end at Br: also terminal, hint again.
    build.commands.push(Command::SetBondMark {
        bond: BondRef::Id(bid(3)),
        mark: Some(StereoMark::Hash {
            narrow: BondEnd::End,
        }),
    });
    // Two centers joined by a wavy bond: a5 (with a6, a7) and a1 (with a2, a3, a4).
    let other = build.atom("C", 200.0, 100.0);
    let x = build.atom("F", 200.0, 70.0);
    let y = build.atom("Cl", 226.0, 115.0);
    build.bond(other, x, BondOrder::Single);
    build.bond(other, y, BondOrder::Single);
    let joint = build.bond(1, other, BondOrder::Single);
    build.mark(joint, StereoMark::Wavy);
    let document = build.document();
    let inspection = all(&document);
    assert_eq!(inspection.misplaced_marks.len(), 2);
    let first = inspection.misplaced_marks[0];
    assert_eq!(
        (
            first.bond,
            first.narrow,
            first.other,
            first.other_could_be_center
        ),
        (bid(1), aid(2), aid(1), true)
    );
    assert_eq!(inspection.wavy_between_centers, [bid(joint)]);
    // The two ends of the wavy bond are items (a mark bears on them) even without records.
    assert_eq!(Plan::of(&document).unwrap().atoms(), [aid(1), aid(5)]);
}

#[test]
fn a_terminal_wedge_whose_other_end_is_terminal_too_has_no_hint() {
    let mut build = Build::default();
    let a = build.atom("O", 0.0, 0.0);
    let b = build.atom("C", 30.0, 0.0);
    let bond = build.bond(a, b, BondOrder::Single);
    build.mark(bond, wedge_begin());
    let inspection = all(&build.document());
    assert_eq!(inspection.misplaced_marks.len(), 1);
    assert!(!inspection.misplaced_marks[0].other_could_be_center);
}

#[test]
fn a_center_with_an_aromatic_bond_is_listed_once_across_instances() {
    // Four neighbors, one of them aromatic, a wedge on another.
    let mut build = Build::default();
    let center = build.atom("C", 100.0, 100.0);
    let ends = [
        build.atom("C", 100.0, 70.0),
        build.atom("C", 126.0, 115.0),
        build.atom("C", 74.0, 115.0),
        build.atom("C", 100.0, 130.0),
    ];
    let bonds = ends.map(|end| build.bond(center, end, BondOrder::Single));
    build.mark(bonds[1], wedge_begin());
    build.commands.push(Command::UpdateBond {
        bond: bid(bonds[2]),
        order: BondOrder::Aromatic,
    });
    build.commands.push(Command::AddInstance {
        source: None,
        offset: Vector { dx: 200.0, dy: 0.0 },
    });
    let inspection = all(&build.document());
    assert_eq!(inspection.atoms[0].drawn.len(), 2);
    assert_eq!(inspection.aromatic_centers, [aid(1)]);
}

#[test]
fn the_aromatic_finding_follows_the_inspected_instances() {
    // The center is derivable only in s2: in s1 a substituent sits on top of it. The finding is
    // about the listed centers that are derivable in an inspected instance, so naming s1 drops
    // it and naming s2, or no instance, keeps it.
    let mut build = Build::default();
    let center = build.atom("C", 100.0, 100.0);
    let ends = [
        build.atom("C", 100.0, 70.0),
        build.atom("C", 126.0, 115.0),
        build.atom("C", 74.0, 115.0),
        build.atom("C", 100.0, 130.0),
    ];
    let bonds = ends.map(|end| build.bond(center, end, BondOrder::Single));
    build.mark(bonds[1], wedge_begin());
    build.commands.push(Command::UpdateBond {
        bond: bid(bonds[2]),
        order: BondOrder::Aromatic,
    });
    build.commands.push(Command::AddInstance {
        source: None,
        offset: Vector { dx: 200.0, dy: 0.0 },
    });
    let mut document = build.document();
    let instances: Vec<ObjectId> = document.instances().collect();
    let [s1, s2] = instances[..] else {
        panic!("two instances")
    };
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![aid(ends[3])],
            delta: Vector { dx: 0.0, dy: -30.0 },
            instance: Some(s1),
        }])
        .unwrap();
    let plan = Plan::of(&document).unwrap();
    let aromatic = |instances: &[ObjectId]| inspect(&document, &plan, instances).unwrap();

    let only_s1 = aromatic(&[s1]);
    assert_eq!(
        only_s1.atoms[0].drawn[0].drawn,
        Err(Underivable::NotDerivable(NotDerivable::CoincidentAtoms))
    );
    assert!(only_s1.aromatic_centers.is_empty());
    assert_eq!(aromatic(&[s2]).aromatic_centers, [aid(center)]);
    assert_eq!(aromatic(&[s1, s2]).aromatic_centers, [aid(center)]);
}

#[test]
fn a_marked_center_the_drawing_cannot_read_is_not_listed_as_aromatic() {
    // Marked but not derivable in any instance and nothing stored: the advice is for centers
    // that were stored or could be derived.
    let mut build = Build::default();
    let center = build.atom("C", 100.0, 100.0);
    let a = build.atom("C", 100.0, 70.0);
    let b = build.atom("C", 126.0, 115.0);
    let c = build.atom("C", 74.0, 115.0);
    build.bond(center, a, BondOrder::Single);
    let wedged = build.bond(center, b, BondOrder::Single);
    build.bond(center, c, BondOrder::Aromatic);
    build.mark(wedged, wedge_begin());
    let inspection = all(&build.document());
    // Three neighbors with an aromatic bond: the fourth position cannot be named.
    assert_eq!(
        inspection.atoms[0].drawn[0].drawn,
        Err(Underivable::NotDerivable(NotDerivable::FourthPosition))
    );
    assert!(inspection.aromatic_centers.is_empty());
}

#[test]
fn a_position_error_of_the_document_is_not_mistaken_for_a_drawing_reason() {
    assert_eq!(
        Underivable::try_from(DeriveError::MissingPosition(aid(1))),
        Err(DeriveError::MissingPosition(aid(1)))
    );
    assert_eq!(
        Underivable::try_from(DeriveError::UnknownAtom(aid(1))),
        Err(DeriveError::UnknownAtom(aid(1)))
    );
    assert_eq!(
        Underivable::try_from(DeriveError::Structure(StereoError::DoubleBondRequired)),
        Ok(Underivable::Structure(StereoError::DoubleBondRequired))
    );
    assert!(matches!(
        InspectError::from(DeriveError::UnknownBond(bid(1))),
        InspectError::Drawing(_)
    ));
}

#[test]
fn the_agreement_names_are_the_documented_five() {
    let names: Vec<&str> = Agreement::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(
        names,
        [
            "agrees",
            "disagrees",
            "only_stored",
            "only_drawn",
            "neither"
        ]
    );
}

#[test]
fn an_inspection_of_a_document_without_items_is_empty() {
    let mut build = Build::default();
    build.halo(100.0, 100.0);
    let document = build.document();
    let plan = Plan::of(&document).unwrap();
    assert!(plan.is_empty());
    let inspection = all(&document);
    assert!(inspection.atoms.is_empty() && inspection.bonds.is_empty());
    assert!(document.stereo().is_empty());
}
