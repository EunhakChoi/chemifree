//! Reading a configuration off a drawing: the sign, the marks that bear on a center, the fourth
//! position, double bonds, and the mirror and flip laws. The expected values are written by hand
//! (vectors in the comments), come from the OpenSMILES definition computed with `atan2`, or come
//! from a public fixture drawing; none of them is produced by the code under test.

use std::collections::BTreeMap;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, Bond, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    EditError, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::TransactionSelection;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoError, StereoKey, StereoMark, StereoState, Turn,
};
use chemifree_domain::stereo_drawing::{
    COLLINEAR_SINE, DEGENERACY_LIMIT, DeriveError, Drawing, MIN_BOND_LENGTH, NotDerivable,
    fourth_position,
};

fn id(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bid(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn slot(serial: u32) -> Slot {
    Slot::Atom(id(serial))
}

fn element(symbol: &str) -> Element {
    Element::from_symbol(symbol).unwrap()
}

fn wedge(narrow: BondEnd) -> Option<StereoMark> {
    Some(StereoMark::Wedge { narrow })
}

fn hash(narrow: BondEnd) -> Option<StereoMark> {
    Some(StereoMark::Hash { narrow })
}

fn tetra(slots: [Slot; 4], turn: Turn) -> CenterState {
    CenterState::tetrahedral(slots, turn).unwrap()
}

/// A drawing under construction: atoms with document positions (y down), bonds with marks.
#[derive(Default)]
struct Sketch {
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    positions: BTreeMap<AtomId, Point>,
}

impl Sketch {
    fn atom(&mut self, serial: u32, symbol: &str, x: f64, y: f64) -> &mut Self {
        self.atom_with(serial, Atom::from(element(symbol)), x, y)
    }

    fn atom_with(&mut self, serial: u32, atom: Atom, x: f64, y: f64) -> &mut Self {
        self.atoms.insert(id(serial), atom);
        self.positions.insert(id(serial), Point::new(x, y).unwrap());
        self
    }

    fn bond(
        &mut self,
        serial: u32,
        begin: u32,
        end: u32,
        order: BondOrder,
        mark: Option<StereoMark>,
    ) -> &mut Self {
        let mut bond = Bond::new(id(begin), id(end), order);
        bond.mark = mark;
        self.bonds.insert(bid(serial), bond);
        self
    }

    /// A single bond from the center `begin` with the mark's narrow end at `begin`.
    fn single(&mut self, serial: u32, begin: u32, end: u32, mark: Option<StereoMark>) -> &mut Self {
        self.bond(serial, begin, end, BondOrder::Single, mark)
    }

    fn drawing(&self) -> Drawing<'_> {
        Drawing::new(
            &self.atoms,
            &self.bonds,
            self.positions.iter().map(|(atom, point)| (*atom, *point)),
        )
    }

    fn center(&self, serial: u32) -> Result<CenterState, DeriveError> {
        self.drawing().derive_center(id(serial))
    }

    fn double_bond(&self, serial: u32) -> Result<DoubleBondState, DeriveError> {
        self.drawing().derive_double_bond(bid(serial))
    }

    /// The same sketch with every position mapped through `map`.
    fn moved(&self, map: impl Fn(f64, f64) -> (f64, f64)) -> Self {
        let positions = self
            .positions
            .iter()
            .map(|(atom, point)| {
                let (x, y) = map(point.x(), point.y());
                (*atom, Point::new(x, y).unwrap())
            })
            .collect();
        Self {
            atoms: self.atoms.clone(),
            bonds: self.bonds.clone(),
            positions,
        }
    }

    /// The same sketch with every bond mark mapped through `map`.
    fn marked(&self, map: impl Fn(StereoMark) -> StereoMark) -> Self {
        let mut copy = Self {
            atoms: self.atoms.clone(),
            bonds: self.bonds.clone(),
            positions: self.positions.clone(),
        };
        for bond in copy.bonds.values_mut() {
            bond.mark = bond.mark.map(&map);
        }
        copy
    }
}

fn refused(result: Result<CenterState, DeriveError>) -> NotDerivable {
    match result {
        Err(DeriveError::NotDerivable(reason)) => reason,
        other => panic!("expected a NotDerivable refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Hand-computed examples. Center C `a1` at (100, 100) in document coordinates (y down). A
// neighbor at document offset (dx, dy) has the frame vector (dx, -dy) / length with z = +1 for
// a wedge and -1 for a hash whose narrow end is the center.
// ---------------------------------------------------------------------------------------------

/// E1: F (0, 30 up) plain; Cl (26, 15 down) wedge; Br (-26, 15 down) plain; fourth = hydrogen.
/// Frame vectors F (0, 1, 0); Cl (0.866, -0.500, +1); Br (-0.866, -0.500, 0); H = minus their
/// sum = (0, 0, -1). D = det[v1-v0, v2-v0, v3-v0] = +3.4647, so clockwise for [F, Cl, Br, H]. By eye: with the hydrogen pointing away, F (top) to Cl (lower right) to Br
/// (lower left) runs clockwise on the screen.
fn e1() -> Sketch {
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 100.0, 100.0)
        .atom(2, "F", 100.0, 70.0)
        .atom(3, "Cl", 126.0, 115.0)
        .atom(4, "Br", 74.0, 115.0)
        .single(1, 1, 2, None)
        .single(2, 1, 3, wedge(BondEnd::Begin))
        .single(3, 1, 4, None);
    sketch
}

fn e1_slots() -> [Slot; 4] {
    [slot(2), slot(3), slot(4), Slot::ImplicitHydrogen]
}

/// E2: `N[C@](Br)(O)C` drawn with N (0, 30 down) as a wedge, Br (0, 30 up), O (-26, 15 down),
/// C (26, 15 down). Vectors N (0, -1, +1); Br (0, 1, 0); O (-0.866, -0.5, 0); C (0.866, -0.5, 0).
/// D = -2.5981, anticlockwise (`@`): seen from the wedged N (the viewer), Br at 12 o'clock to O
/// at 8 o'clock to C at 4 o'clock runs anticlockwise. Ids follow the SMILES order a2..a5.
fn e2_with(n_id: u32, br_id: u32, o_id: u32, c_id: u32, swap_o_and_c: bool) -> Sketch {
    let (o_x, c_x) = if swap_o_and_c {
        (126.0, 74.0)
    } else {
        (74.0, 126.0)
    };
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 100.0, 100.0)
        .atom(n_id, "N", 100.0, 130.0)
        .atom(br_id, "Br", 100.0, 70.0)
        .atom(o_id, "O", o_x, 115.0)
        .atom(c_id, "C", c_x, 115.0)
        .single(1, 1, n_id, wedge(BondEnd::Begin))
        .single(2, 1, br_id, None)
        .single(3, 1, o_id, None)
        .single(4, 1, c_id, None);
    sketch
}

fn quad(a: u32, b: u32, c: u32, d: u32) -> [Slot; 4] {
    [slot(a), slot(b), slot(c), slot(d)]
}

#[test]
fn e1_is_clockwise_over_f_cl_br_and_the_hydrogen() {
    assert_eq!(e1().center(1), Ok(tetra(e1_slots(), Turn::Clockwise)));
}

#[test]
fn the_opensmiles_anchor_n_c_at_br_o_c_is_anticlockwise_and_at_at_is_clockwise() {
    let sketch = e2_with(2, 3, 4, 5, false);
    assert_eq!(
        sketch.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Anticlockwise))
    );
    // `N[C@@](Br)(O)C`: O and C exchanged on the page.
    let mirrored_picture = e2_with(2, 3, 4, 5, true);
    assert_eq!(
        mirrored_picture.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise))
    );
}

#[test]
fn e2_with_br_created_first_is_one_swap_from_the_smiles_order() {
    // Br a2, N a3, O a4, C a5: the canonical order [Br, N, O, C] is one transposition of the
    // SMILES order [N, Br, O, C], so the stored turn is clockwise.
    let sketch = e2_with(3, 2, 4, 5, false);
    assert_eq!(
        sketch.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise))
    );
}

/// Inversions of `order` counted pair by pair, a different method from the production helper.
fn inversions(order: &[usize]) -> usize {
    let mut count = 0;
    for (index, earlier) in order.iter().enumerate() {
        for later in &order[index + 1..] {
            if earlier > later {
                count += 1;
            }
        }
    }
    count
}

fn permutations_of_four() -> Vec<[usize; 4]> {
    let mut all = Vec::new();
    for a in 0..4 {
        for b in (0..4).filter(|&b| b != a) {
            for c in (0..4).filter(|&c| c != a && c != b) {
                all.push([a, b, c, 6 - a - b - c]);
            }
        }
    }
    assert_eq!(all.len(), 24);
    all
}

#[test]
fn all_24_creation_orders_flip_the_turn_exactly_when_the_permutation_is_odd() {
    // Roles in SMILES order: 0 = N, 1 = Br, 2 = O, 3 = C. Role r gets atom id `2 + ids[r]`.
    // The canonical order lists the roles by ascending id, i.e. the inverse permutation of
    // `ids`; the SMILES order has turn anticlockwise, so the stored turn is anticlockwise
    // flipped once per inversion of that role sequence.
    for ids in permutations_of_four() {
        let id_of = |role: usize| u32::try_from(2 + ids[role]).unwrap();
        let sketch = e2_with(id_of(0), id_of(1), id_of(2), id_of(3), false);
        let mut roles_by_id = [0_usize; 4];
        for (role, position) in ids.iter().enumerate() {
            roles_by_id[*position] = role;
        }
        let expected = if inversions(&roles_by_id) % 2 == 1 {
            Turn::Clockwise
        } else {
            Turn::Anticlockwise
        };
        assert_eq!(
            sketch.center(1),
            Ok(tetra(quad(2, 3, 4, 5), expected)),
            "creation order {ids:?}"
        );
    }
}

#[test]
fn e3_moving_the_bromine_across_the_gap_flips_the_turn() {
    // E1 with Br at (126, 85): frame vector (0.866, 0.5, 0); D = -3.4647.
    let mut sketch = e1();
    sketch.atom(4, "Br", 126.0, 85.0);
    assert_eq!(sketch.center(1), Ok(tetra(e1_slots(), Turn::Anticlockwise)));
}

/// E4 (a cross): N (0, 1, 0), E (1, 0, 0), S wedge (0, -1, +1), W hash (-1, 0, -1), D = +4.0.
fn cross(south: Option<StereoMark>, west: Option<StereoMark>) -> Sketch {
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 100.0, 100.0)
        .atom(2, "C", 100.0, 70.0)
        .atom(3, "C", 130.0, 100.0)
        .atom(4, "C", 100.0, 130.0)
        .atom(5, "C", 70.0, 100.0)
        .single(1, 1, 2, None)
        .single(2, 1, 3, None)
        .single(3, 1, 4, south)
        .single(4, 1, 5, west);
    sketch
}

#[test]
fn e4_a_cross_with_a_wedge_and_a_hash_next_to_each_other_is_clockwise() {
    let sketch = cross(wedge(BondEnd::Begin), hash(BondEnd::Begin));
    assert_eq!(
        sketch.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise))
    );
}

#[test]
fn e5_a_wedge_opposite_a_hash_with_two_plain_bonds_is_degenerate() {
    // N wedge (0, 1, +1), S hash (0, -1, -1), E and W plain: D = 0.
    let mut sketch = cross(hash(BondEnd::Begin), None);
    sketch.single(1, 1, 2, wedge(BondEnd::Begin));
    assert_eq!(refused(sketch.center(1)), NotDerivable::Degenerate);
}

#[test]
fn a_narrow_end_at_the_other_atom_or_on_the_end_side_both_work() {
    // The same wedge written from the other atom: bond begins at Cl, narrow end is `End` = the
    // center. The picture and the answer are the same as E1.
    let mut sketch = e1();
    sketch.single(2, 3, 1, wedge(BondEnd::End));
    assert_eq!(sketch.center(1), Ok(tetra(e1_slots(), Turn::Clockwise)));
}

#[test]
fn a_wedge_whose_narrow_end_is_the_neighbor_says_nothing_about_the_center() {
    let mut sketch = e1();
    sketch.single(2, 1, 3, wedge(BondEnd::End));
    assert_eq!(refused(sketch.center(1)), NotDerivable::NoMarks);
}

#[test]
fn an_unmarked_center_states_nothing() {
    let mut plain = e1();
    plain.single(2, 1, 3, None);
    assert_eq!(refused(plain.center(1)), NotDerivable::NoMarks);
}

#[test]
fn the_sign_reverses_when_every_y_is_negated() {
    // Regression for the one place that handedness can be lost: the y flip. The same pictures
    // with y negated are the mirror images, so the documented signs must reverse.
    let negated_y = |sketch: &Sketch| sketch.moved(|x, y| (x, -y));
    assert_eq!(
        negated_y(&e1()).center(1),
        Ok(tetra(e1_slots(), Turn::Anticlockwise))
    );
    assert_eq!(
        negated_y(&e2_with(2, 3, 4, 5, false)).center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise))
    );
}

// ---------------------------------------------------------------------------------------------
// Public fixture: the authored drawing phase3-stereo-and-missing-number.cdxml of the public
// functional specification, typed here by hand (not parsed). It draws F[C@H](Cl)[C@H](Br)I with
// one tapered hash per center; an independent reader (RDKit) reads it as that SMILES.
// ---------------------------------------------------------------------------------------------

/// Atoms: a1 F (0, 40), a2 C (0, 0), a3 Cl (-35, -18), a4 C (35, -18), a5 Br (35, -58),
/// a6 I (70, 0). Bonds: b1 F-C2 hash, narrow end at a2 (`End`); b2 C2-Cl; b3 C2-C4; b4 C4-Br
/// hash, narrow end at a4 (`Begin`); b5 C4-I.
/// Center a2, frame vectors F (0, -1, -1), Cl (-0.889, 0.457, 0), C4 (0.889, 0.457, 0), hydrogen
/// (0, 0.086, 1): D = -3.2537, anticlockwise for [F, Cl, C4, H]. Center a4: C2 (-0.889, -0.457, 0),
/// Br (0, 1, -1), I (0.889, -0.457, 0), hydrogen (0, -0.086, 1): D = -3.2537 for [C2, Br, I, H].
fn public_fixture() -> Sketch {
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "F", 0.0, 40.0)
        .atom(2, "C", 0.0, 0.0)
        .atom(3, "Cl", -35.0, -18.0)
        .atom(4, "C", 35.0, -18.0)
        .atom(5, "Br", 35.0, -58.0)
        .atom(6, "I", 70.0, 0.0)
        .single(1, 1, 2, hash(BondEnd::End))
        .single(2, 2, 3, None)
        .single(3, 2, 4, None)
        .single(4, 4, 5, hash(BondEnd::Begin))
        .single(5, 4, 6, None);
    sketch
}

#[test]
fn the_public_two_center_drawing_reads_as_f_c_at_h_cl_c_at_h_br_i() {
    let sketch = public_fixture();
    let h = Slot::ImplicitHydrogen;
    assert_eq!(
        sketch.center(2),
        Ok(tetra([slot(1), slot(3), slot(4), h], Turn::Anticlockwise))
    );
    assert_eq!(
        sketch.center(4),
        Ok(tetra([slot(2), slot(5), slot(6), h], Turn::Anticlockwise))
    );
    // SMILES order F, H, Cl, C4 is an even permutation of the canonical order: `@`.
    let first = sketch.center(2).unwrap();
    assert_eq!(
        first.turn_for_order([slot(1), h, slot(3), slot(4)]),
        Ok(Some(Turn::Anticlockwise))
    );
    // Without the y flip both would read `@@`: the same picture with y negated.
    let negated = sketch.moved(|x, y| (x, -y));
    assert_eq!(
        negated.center(2),
        Ok(tetra([slot(1), slot(3), slot(4), h], Turn::Clockwise))
    );
}

#[test]
fn a_cdx_bond_ordering_with_one_empty_entry_maps_to_slots_and_a_turn() {
    // The authored drawing lists the first center's bond ordering as `8 9 0 10`: three bond
    // identifiers and one empty entry. Here the bonds are b1 (F), b2 (Cl), b3 (C4) and the empty
    // entry is the record's own non-atom slot (the code does not say what it is; the record
    // does). Mapping each bond to its far atom gives [F, Cl, (non-atom), C4], one transposition
    // of the canonical [F, Cl, C4, non-atom]: the turn over that order is clockwise.
    let sketch = public_fixture();
    let state = sketch.center(2).unwrap();
    let far_atom = |bond: u32| {
        let bond = &sketch.bonds[&bid(bond)];
        if bond.begin == id(2) {
            bond.end
        } else {
            bond.begin
        }
    };
    let record_non_atom = *state
        .slots()
        .iter()
        .find(|slot| !matches!(slot, Slot::Atom(_)))
        .expect("a three-neighbor center has a non-atom slot");
    let order = [
        Slot::Atom(far_atom(1)),
        Slot::Atom(far_atom(2)),
        record_non_atom,
        Slot::Atom(far_atom(3)),
    ];
    assert_eq!(state.turn_for_order(order), Ok(Some(Turn::Clockwise)));
    // And stating that turn over that order stores the derived record.
    assert_eq!(CenterState::tetrahedral(order, Turn::Clockwise), Ok(state));
}

#[test]
fn the_specs_positive_negative_pair_on_f_cl_br_and_a_hydrogen() {
    // Public model: positive iff det(p0-p3, p1-p3, p2-p3) > 0, with the ligands [F, Cl, Br,
    // implicit hydrogen]. E1 is clockwise (negative); reflecting the picture gives the pair's
    // other member (positive = anticlockwise), computed by the public formula on hand vectors.
    let positive = e1().moved(|x, y| (200.0 - x, y));
    assert_eq!(
        positive.center(1),
        Ok(tetra(e1_slots(), Turn::Anticlockwise))
    );
    assert_eq!(e1().center(1), Ok(tetra(e1_slots(), Turn::Clockwise)));
}

// ---------------------------------------------------------------------------------------------
// Definition-level property test. The expected turn comes from the OpenSMILES definition
// (look from the first neighbor toward the center; do the others run anticlockwise?) computed
// with projection and `atan2`, which shares no formula with the determinant in the code.
// ---------------------------------------------------------------------------------------------

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    #[expect(clippy::cast_precision_loss, reason = "53 random bits are enough")]
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

type Vec3 = [f64; 3];

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn det3(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    dot(a, cross3(b, c))
}

fn minus(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unit3(a: Vec3) -> Vec3 {
    let length = dot(a, a).sqrt();
    [a[0] / length, a[1] / length, a[2] / length]
}

/// The OpenSMILES reading of four vectors: `Some(true)` when, seen from the first, the other
/// three run anticlockwise. `None` when two projected directions are too close to tell.
fn anticlockwise_by_definition(v: &[Vec3; 4]) -> Option<bool> {
    // The viewer sits at +n looking toward the origin; (e1, e2, n) is right-handed, so an
    // increasing angle in the (e1, e2) plane is anticlockwise for the viewer.
    let n = unit3(v[0]);
    let helper = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let e1 = unit3(cross3(helper, n));
    let e2 = cross3(n, e1);
    let tau = std::f64::consts::TAU;
    let angle = |w: Vec3| dot(w, e2).atan2(dot(w, e1));
    let base = angle(v[1]);
    let second = (angle(v[2]) - base).rem_euclid(tau);
    let third = (angle(v[3]) - base).rem_euclid(tau);
    if second < 1e-3 || third < 1e-3 || (second - third).abs() < 1e-3 {
        return None;
    }
    Some(second < third)
}

/// A random picture: frame vectors with lifts, the sketch that draws them, and whether the
/// four vectors strictly enclose the origin.
struct Case {
    vectors: [Vec3; 4],
    sketch: Sketch,
    neighbors: u32,
}

fn random_case(random: &mut Random, neighbors: u32) -> Option<Case> {
    let count = usize::try_from(neighbors).unwrap();
    let mut planar = Vec::new();
    let mut vectors: Vec<Vec3> = Vec::new();
    let mut lifts = Vec::new();
    for _ in 0..count {
        let angle = random.unit() * std::f64::consts::TAU;
        let lift = [-1.0, 0.0, 1.0][usize::try_from(random.below(3)).unwrap()];
        let length = 20.0 + 40.0 * random.unit();
        planar.push((angle, length));
        lifts.push(lift);
        vectors.push([angle.cos(), angle.sin(), lift]);
    }
    if count == 3 {
        let w = [
            -(vectors[0][0] + vectors[1][0] + vectors[2][0]),
            -(vectors[0][1] + vectors[1][1] + vectors[2][1]),
            -(vectors[0][2] + vectors[1][2] + vectors[2][2]),
        ];
        vectors.push(w);
    }
    let v: [Vec3; 4] = vectors.try_into().unwrap();
    // Barycentric weights of the origin: the signed volumes of the four sub-tetrahedra. All of
    // one sign (and not tiny) means the origin is inside.
    let weights = [
        det3(v[1], v[2], v[3]),
        -det3(v[0], v[2], v[3]),
        det3(v[0], v[1], v[3]),
        -det3(v[0], v[1], v[2]),
    ];
    let total: f64 = weights.iter().map(|w| w.abs()).sum();
    let same_sign = weights.iter().all(|w| *w > 0.0) || weights.iter().all(|w| *w < 0.0);
    if !same_sign || total < 0.05 {
        return None;
    }
    let mut sketch = Sketch::default();
    sketch.atom(1, "C", 300.0, 300.0);
    for (index, ((angle, length), lift)) in planar.iter().zip(&lifts).enumerate() {
        let serial = u32::try_from(index).unwrap() + 2;
        // Document y is down: the test flips y itself, independently of the production code.
        sketch.atom(
            serial,
            "C",
            300.0 + length * angle.cos(),
            300.0 - length * angle.sin(),
        );
        let mark = match (*lift > 0.0, *lift < 0.0, random.below(2)) {
            (true, _, 0) => wedge(BondEnd::Begin),
            (true, _, _) => wedge(BondEnd::End),
            (_, true, 0) => hash(BondEnd::Begin),
            (_, true, _) => hash(BondEnd::End),
            _ => None,
        };
        // The narrow end is the center: begin = center for `Begin`, begin = neighbor for `End`.
        let narrow_at_end = matches!(
            mark,
            Some(
                StereoMark::Wedge {
                    narrow: BondEnd::End
                } | StereoMark::Hash {
                    narrow: BondEnd::End
                }
            )
        );
        if narrow_at_end {
            sketch.single(serial - 1, serial, 1, mark);
        } else {
            sketch.single(serial - 1, 1, serial, mark);
        }
    }
    Some(Case {
        vectors: v,
        sketch,
        neighbors,
    })
}

fn case_slots(neighbors: u32) -> [Slot; 4] {
    if neighbors == 3 {
        [slot(2), slot(3), slot(4), Slot::ImplicitHydrogen]
    } else {
        quad(2, 3, 4, 5)
    }
}

fn rotated(sketch: &Sketch, degrees: f64) -> Sketch {
    let (sin, cos) = degrees.to_radians().sin_cos();
    sketch.moved(|x, y| {
        let (dx, dy) = (x - 300.0, y - 300.0);
        (300.0 + dx * cos - dy * sin, 300.0 + dx * sin + dy * cos)
    })
}

fn flipped_over(sketch: &Sketch) -> Sketch {
    sketch.marked(StereoMark::flipped_over)
}

fn reflected(sketch: &Sketch) -> Sketch {
    sketch.moved(|x, y| (600.0 - x, y))
}

#[test]
fn a_seeded_property_test_agrees_with_the_opensmiles_definition() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    let mut checked = [0_u32; 2];
    let mut attempts = 0_u32;
    while checked[0] < 1_200 || checked[1] < 1_200 {
        attempts += 1;
        assert!(attempts < 400_000, "the generator keeps rejecting cases");
        let neighbors = if checked[0] < 1_200 && (checked[1] >= 1_200 || random.below(2) == 0) {
            3
        } else {
            4
        };
        let Some(case) = random_case(&mut random, neighbors) else {
            continue;
        };
        let Some(anticlockwise) = anticlockwise_by_definition(&case.vectors) else {
            continue;
        };
        let expected_turn = if anticlockwise {
            Turn::Anticlockwise
        } else {
            Turn::Clockwise
        };
        let slots = case_slots(case.neighbors);
        let expected = tetra(slots, expected_turn);
        let derived = case.sketch.center(1);
        assert_eq!(derived, Ok(expected), "case {attempts}: {:?}", case.vectors);

        // The public model: positive iff det(p0-p3, p1-p3, p2-p3) > 0, and positive is
        // anticlockwise.
        let v = &case.vectors;
        let public = det3(minus(v[0], v[3]), minus(v[1], v[3]), minus(v[2], v[3]));
        assert_eq!(
            public > 0.0,
            anticlockwise,
            "public-model formula, case {attempts}"
        );

        // Laws on the same picture.
        assert_eq!(
            reflected(&case.sketch).center(1),
            Ok(expected.mirrored()),
            "reflection mirrors, case {attempts}"
        );
        assert_eq!(
            flipped_over(&reflected(&case.sketch)).center(1),
            Ok(expected),
            "reflection with wedge and hash swapped keeps the configuration, case {attempts}"
        );
        let degrees = f64::from(u32::try_from(random.below(360)).unwrap());
        assert_eq!(
            rotated(&case.sketch, degrees).center(1),
            Ok(expected),
            "rotation by {degrees}, case {attempts}"
        );
        assert_eq!(
            case.sketch.moved(|x, y| (x + 1234.5, y - 987.25)).center(1),
            Ok(expected),
            "translation, case {attempts}"
        );
        checked[usize::try_from(case.neighbors - 3).unwrap()] += 1;
    }
    assert!(checked[0] + checked[1] >= 2_000, "checked {checked:?}");
}

#[test]
fn rotating_every_hand_example_by_each_whole_degree_keeps_its_configuration() {
    let examples: [(Sketch, Result<CenterState, DeriveError>); 3] = [
        (e1(), Ok(tetra(e1_slots(), Turn::Clockwise))),
        (
            e2_with(2, 3, 4, 5, false),
            Ok(tetra(quad(2, 3, 4, 5), Turn::Anticlockwise)),
        ),
        (
            cross(wedge(BondEnd::Begin), hash(BondEnd::Begin)),
            Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise)),
        ),
    ];
    for (sketch, expected) in &examples {
        for degrees in 0..360 {
            assert_eq!(
                &rotated(sketch, f64::from(degrees)).center(1),
                expected,
                "rotation by {degrees}"
            );
        }
    }
}

#[test]
fn bond_lengths_do_not_matter_only_directions() {
    let mut longer = e1();
    longer
        .atom(2, "F", 100.0, 100.0 - 300.0)
        .atom(3, "Cl", 100.0 + 2.6, 100.0 + 1.5);
    assert_eq!(longer.center(1), Ok(tetra(e1_slots(), Turn::Clockwise)));
}

#[test]
fn scaling_every_lift_would_scale_the_orientation_not_change_its_sign() {
    // Documents the homogeneity the module relies on: D for E1 is 3.4647 at lift 1. With the
    // vectors of E1 and lift s, D(s) = 3.4647 * s for s > 0 (hand computation: the lifted
    // components are the only ones that change, and D is linear in them).
    let vector = |s: f64| -> [Vec3; 4] {
        let (f, cl, br) = (
            [0.0, 1.0, 0.0],
            [0.866_025_403_784_438_6, -0.5, s],
            [-0.866_025_403_784_438_6, -0.5, 0.0],
        );
        let h = [
            -(f[0] + cl[0] + br[0]),
            -(f[1] + cl[1] + br[1]),
            -(f[2] + cl[2] + br[2]),
        ];
        [f, cl, br, h]
    };
    let d = |s: f64| {
        let v = vector(s);
        det3(minus(v[1], v[0]), minus(v[2], v[0]), minus(v[3], v[0]))
    };
    assert!((d(1.0) - 3.4647).abs() < 1e-3, "D(1) = {}", d(1.0));
    assert!((d(0.1) - 0.3465).abs() < 1e-3, "D(0.1) = {}", d(0.1));
    assert!((d(10.0) - 34.647).abs() < 1e-2, "D(10) = {}", d(10.0));
}

// ---------------------------------------------------------------------------------------------
// Marks: wavy, conflicts, scope.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_wavy_bond_alone_makes_the_center_unknown() {
    let mut sketch = e1();
    sketch.single(2, 1, 3, Some(StereoMark::Wavy));
    assert_eq!(
        sketch.center(1),
        Ok(CenterState::unknown(e1_slots()).unwrap())
    );
}

#[test]
fn an_unknown_center_reads_no_geometry() {
    // Two neighbors on top of each other would be refused for a tetrahedral reading; an unknown
    // statement reads no position at all.
    let mut sketch = e1();
    sketch.single(2, 1, 3, Some(StereoMark::Wavy));
    sketch.atom(4, "Br", 126.0, 115.0);
    assert_eq!(
        sketch.center(1),
        Ok(CenterState::unknown(e1_slots()).unwrap())
    );
}

#[test]
fn a_wavy_bond_at_the_center_with_a_wedge_of_its_own_is_a_conflict() {
    let mut sketch = e1();
    sketch.single(3, 1, 4, Some(StereoMark::Wavy));
    assert_eq!(refused(sketch.center(1)), NotDerivable::ConflictingMarks);
}

/// Two possible centers `a1` and `a2` joined by a wavy bond `b1`, each with two more neighbors.
fn wavy_pair(a1_mark: Option<StereoMark>, a2_mark: Option<StereoMark>) -> Sketch {
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 100.0, 100.0)
        .atom(2, "C", 140.0, 100.0)
        .atom(3, "F", 80.0, 70.0)
        .atom(4, "Cl", 80.0, 130.0)
        .atom(5, "Br", 160.0, 70.0)
        .atom(6, "I", 160.0, 130.0)
        .single(1, 1, 2, Some(StereoMark::Wavy))
        .single(2, 1, 3, a1_mark)
        .single(3, 1, 4, None)
        .single(4, 2, 5, a2_mark)
        .single(5, 2, 6, None);
    sketch
}

#[test]
fn a_wavy_bond_between_two_possible_centers_follows_each_end_s_own_mark() {
    let sketch = wavy_pair(wedge(BondEnd::Begin), None);
    // a1 has its own wedge, so it follows it exactly as if the wavy bond were plain.
    let mut plain = sketch.marked(|mark| mark);
    plain.bonds.get_mut(&bid(1)).unwrap().mark = None;
    let a1 = sketch.center(1).unwrap();
    assert_eq!(Ok(a1), plain.center(1));
    assert!(matches!(a1, CenterState::Tetrahedral { .. }));
    // a2 has no mark of its own: unknown.
    assert!(matches!(sketch.center(2), Ok(CenterState::Unknown { .. })));
}

#[test]
fn an_explicitly_named_end_of_a_wavy_bond_without_a_mark_of_its_own_is_unknown() {
    let sketch = wavy_pair(None, None);
    assert!(matches!(sketch.center(1), Ok(CenterState::Unknown { .. })));
    assert!(matches!(sketch.center(2), Ok(CenterState::Unknown { .. })));
}

#[test]
fn a_wavy_bond_to_an_sp2_carbon_with_three_neighbors_counts_as_a_possible_center() {
    // Pins a deliberate structural approximation: whether the other end of a wavy bond "can be
    // a center" is only a neighbor-count and element test (a non-query atom with 3 or 4
    // neighbors), not a chemical one. a2 is an sp2 carbon (a double bond to a5, a single bond
    // to a6, and the wavy bond), which is no stereocenter, but it has three neighbors, so the
    // wavy bond is not read as a conflict at a1: a1 follows its own wedge.
    let mut sketch = wavy_pair(wedge(BondEnd::Begin), None);
    sketch.bond(4, 2, 5, BondOrder::Double, None);
    let mut plain = sketch.marked(|mark| mark);
    plain.bonds.get_mut(&bid(1)).unwrap().mark = None;
    let a1 = sketch.center(1);
    assert!(
        matches!(a1, Ok(CenterState::Tetrahedral { .. })),
        "no conflict with an sp2 neighbor: {a1:?}"
    );
    assert_eq!(a1, plain.center(1), "it reads as if the bond were plain");
    // With only two neighbors the other end cannot be a center, and the same drawing conflicts.
    let mut terminal = sketch.marked(|mark| mark);
    terminal.bonds.remove(&bid(5));
    assert_eq!(refused(terminal.center(1)), NotDerivable::ConflictingMarks);
}

#[test]
fn a_wedge_on_a_bond_that_cannot_carry_it_is_no_mark_at_all() {
    // A document never holds this (a wedge fits a single bond only), but `Drawing::new` takes
    // arbitrary maps. Such a mark neither lifts the neighbor nor counts as a mark.
    let mut on_double = cross(wedge(BondEnd::Begin), None);
    on_double.bonds.get_mut(&bid(3)).unwrap().order = BondOrder::Double;
    assert_eq!(refused(on_double.center(1)), NotDerivable::NoMarks);
    // A wedge to the south and a hash to the north cancel to a flat picture (D = 0) when both
    // count. With the wedge on a triple bond only the hash lifts: anticlockwise, D = -2 by hand
    // (N (0, 1, -1), E (1, 0, 0), S (0, -1, 0), W (-1, 0, 0)).
    let mut ignored_wedge = cross(wedge(BondEnd::Begin), None);
    ignored_wedge.bonds.get_mut(&bid(1)).unwrap().mark = hash(BondEnd::Begin);
    assert_eq!(
        refused(ignored_wedge.center(1)),
        NotDerivable::Degenerate,
        "both marks valid: flat"
    );
    ignored_wedge.bonds.get_mut(&bid(3)).unwrap().order = BondOrder::Triple;
    assert_eq!(
        ignored_wedge.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Anticlockwise))
    );
}

#[test]
fn a_wavy_mark_on_a_bond_that_cannot_carry_it_does_not_make_the_center_unknown() {
    let mut on_double = cross(None, None);
    on_double.bonds.get_mut(&bid(1)).unwrap().order = BondOrder::Double;
    on_double.bonds.get_mut(&bid(1)).unwrap().mark = Some(StereoMark::Wavy);
    assert_eq!(refused(on_double.center(1)), NotDerivable::NoMarks);
    // And it is no conflict beside a valid wedge either: the wedge is read on its own.
    let mut beside_wedge = cross(wedge(BondEnd::Begin), None);
    beside_wedge.bonds.get_mut(&bid(1)).unwrap().order = BondOrder::Double;
    beside_wedge.bonds.get_mut(&bid(1)).unwrap().mark = Some(StereoMark::Wavy);
    assert_eq!(
        beside_wedge.center(1),
        Ok(tetra(quad(2, 3, 4, 5), Turn::Clockwise)),
        "the wedge alone: D = +2 by hand"
    );
}

#[test]
fn a_wavy_single_bond_beside_a_double_bond_is_not_read_as_either_e_or_z() {
    // Deliberate limit: the usual "either" marker on a bond next to a double bond states
    // nothing about the double bond here.
    let mut sketch = zigzag(false);
    sketch.bond(9, 1, 6, BondOrder::Single, Some(StereoMark::Wavy));
    sketch.atom(6, "C", 70.0, 85.0);
    assert!(matches!(
        sketch.double_bond(2),
        Ok(DoubleBondState::Relative { .. })
    ));
}

// ---------------------------------------------------------------------------------------------
// Structural refusals and the order they are reported in.
// ---------------------------------------------------------------------------------------------

#[test]
fn only_three_or_four_neighbors_and_no_query_atom_can_be_a_center() {
    let mut two = Sketch::default();
    two.atom(1, "C", 0.0, 0.0)
        .atom(2, "C", 10.0, 0.0)
        .atom(3, "C", 0.0, 10.0)
        .single(1, 1, 2, wedge(BondEnd::Begin))
        .single(2, 1, 3, None);
    assert_eq!(
        two.center(1),
        Err(DeriveError::Structure(StereoError::CenterUnsupported))
    );
    let mut five = cross(wedge(BondEnd::Begin), None);
    five.atom(6, "C", 130.0, 130.0).single(5, 1, 6, None);
    assert_eq!(
        five.center(1),
        Err(DeriveError::Structure(StereoError::CenterUnsupported))
    );
    let mut query = e1();
    query.atom_with(1, Atom::new(AtomKind::Query(QueryAtom::Any)), 100.0, 100.0);
    assert_eq!(
        query.center(1),
        Err(DeriveError::Structure(StereoError::CenterUnsupported))
    );
    assert_eq!(
        e1().drawing().derive_center(id(9)),
        Err(DeriveError::UnknownAtom(id(9)))
    );
}

#[test]
fn a_neighbor_without_a_position_is_reported_not_guessed() {
    let mut sketch = e1();
    sketch.positions.remove(&id(4));
    assert_eq!(sketch.center(1), Err(DeriveError::MissingPosition(id(4))));
}

#[test]
fn a_query_atom_neighbor_is_an_ordinary_neighbor() {
    let mut sketch = e1();
    sketch.atom_with(
        4,
        Atom::new(AtomKind::Query(QueryAtom::Halogen)),
        74.0,
        115.0,
    );
    assert_eq!(sketch.center(1), Ok(tetra(e1_slots(), Turn::Clockwise)));
}

// ---------------------------------------------------------------------------------------------
// Constants: both sides of each limit.
// ---------------------------------------------------------------------------------------------

/// Plain A and B nearly opposite on the x axis (B tilted so that |D| = `target`), a wedge W up:
/// frame vectors A (-1, 0, 0), B (cos t, sin t, 0), W (0, 1, +1). det[A, B, W] = -sin t, so
/// |D| = 4 sin t.
fn nearly_straight(target_d: f64) -> Sketch {
    let tilt = (target_d / 4.0).asin();
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 0.0, 0.0)
        .atom(2, "C", -50.0, 0.0)
        .atom(3, "C", 50.0 * tilt.cos(), -50.0 * tilt.sin())
        .atom(4, "C", 0.0, -40.0)
        .single(1, 1, 2, None)
        .single(2, 1, 3, None)
        .single(3, 1, 4, wedge(BondEnd::Begin));
    sketch
}

#[test]
fn the_degenerate_limit_flips_exactly_at_the_constant() {
    assert_eq!(
        refused(nearly_straight(0.5 * DEGENERACY_LIMIT).center(1)),
        NotDerivable::Degenerate
    );
    assert!(matches!(
        nearly_straight(2.0 * DEGENERACY_LIMIT).center(1),
        Ok(CenterState::Tetrahedral { .. })
    ));
}

#[test]
fn a_nearly_straight_pair_with_a_wedge_still_gives_a_confident_answer_above_the_limit() {
    // Documented risk: |D| = 4 * tilt, so a picture flat to a few hundredths of a degree is
    // answered, and the answer flips with the side of the tilt.
    let up = nearly_straight(0.01).center(1).unwrap();
    let down = nearly_straight(-0.01).center(1).unwrap();
    assert_eq!(up, down.mirrored());
}

#[test]
fn coincident_neighbors_are_refused_at_both_sides_of_the_minimum_length() {
    let mut close = e1();
    close.atom(3, "Cl", 100.0 + 0.5 * MIN_BOND_LENGTH, 100.0);
    assert_eq!(refused(close.center(1)), NotDerivable::CoincidentAtoms);
    let mut apart = e1();
    apart.atom(3, "Cl", 100.0 + 2.0 * MIN_BOND_LENGTH, 100.0);
    assert_ne!(
        apart.center(1),
        Err(DeriveError::NotDerivable(NotDerivable::CoincidentAtoms))
    );
}

// ---------------------------------------------------------------------------------------------
// The fourth position, row by row.
// ---------------------------------------------------------------------------------------------

fn atom_of(symbol: &str, charge: i8, hydrogens: Hydrogens, radical: Radical) -> Atom {
    Atom {
        charge,
        hydrogens,
        radical,
        ..Atom::from(element(symbol))
    }
}

const S: Option<u16> = Some(1);
const D: Option<u16> = Some(2);
const NONE: Option<u16> = None;

/// A name, the atom, its incident bond contributions, and the expected fourth position.
type Row = (
    &'static str,
    Atom,
    Vec<Option<u16>>,
    Result<Option<Slot>, NotDerivable>,
);

#[test]
fn the_fourth_position_of_a_three_neighbor_center_by_table() {
    use Hydrogens::{Inferred, Specified, Unknown};
    let h = Ok(Some(Slot::ImplicitHydrogen));
    let lone = Ok(Some(Slot::LonePair));
    let no = Err(NotDerivable::FourthPosition);
    let rows: Vec<Row> = vec![
        (
            "C inferred",
            atom_of("C", 0, Inferred, Radical::None),
            vec![S, S, S],
            h,
        ),
        (
            "C stated 1",
            atom_of("C", 0, Specified(1), Radical::None),
            vec![S, S, S],
            h,
        ),
        (
            "C stated 0 contradicted by valence",
            atom_of("C", 0, Specified(0), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "C stated 2",
            atom_of("C", 0, Specified(2), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "sp2 C (one double bond)",
            atom_of("C", 0, Inferred, Radical::None),
            vec![S, S, D],
            no,
        ),
        (
            "carbocation",
            atom_of("C", 1, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "carbanion",
            atom_of("C", -1, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "boron",
            atom_of("B", 0, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "radical carbon",
            atom_of("C", 0, Inferred, Radical::Doublet),
            vec![S, S, S],
            no,
        ),
        (
            "hydrogens unknown",
            atom_of("C", 0, Unknown, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "aromatic neighbor, inferred",
            atom_of("C", 0, Inferred, Radical::None),
            vec![NONE, S, S],
            no,
        ),
        (
            "aromatic neighbor, stated",
            atom_of("C", 0, Specified(1), Radical::None),
            vec![NONE, S, S],
            no,
        ),
        (
            "amine N",
            atom_of("N", 0, Inferred, Radical::None),
            vec![S, S, S],
            lone,
        ),
        (
            "amine N stated 0",
            atom_of("N", 0, Specified(0), Radical::None),
            vec![S, S, S],
            lone,
        ),
        (
            "N stated 1 contradicted by valence",
            atom_of("N", 0, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "ammonium N+",
            atom_of("N", 1, Inferred, Radical::None),
            vec![S, S, S],
            h,
        ),
        (
            "hypervalent N (sum 4)",
            atom_of("N", 0, Inferred, Radical::None),
            vec![S, S, D],
            no,
        ),
        (
            "hypervalent N (sum 5)",
            atom_of("N", 0, Inferred, Radical::None),
            vec![S, D, D],
            no,
        ),
        (
            "N stated 0 sum 5",
            atom_of("N", 0, Specified(0), Radical::None),
            vec![S, D, D],
            no,
        ),
        (
            "phosphine P",
            atom_of("P", 0, Inferred, Radical::None),
            vec![S, S, S],
            lone,
        ),
        (
            "P(V) with a double bond",
            atom_of("P", 0, Inferred, Radical::None),
            vec![S, S, D],
            no,
        ),
        (
            "amine N with an attachment point",
            Atom {
                attachment_points: AttachmentPoints::new(&[1]).unwrap(),
                ..atom_of("N", 0, Specified(0), Radical::None)
            },
            vec![S, S, S],
            no,
        ),
        (
            "sulfoxide S",
            atom_of("S", 0, Inferred, Radical::None),
            vec![S, S, D],
            lone,
        ),
        (
            "sulfoxide S stated 0",
            atom_of("S", 0, Specified(0), Radical::None),
            vec![S, S, D],
            lone,
        ),
        (
            "S with three single bonds",
            atom_of("S", 0, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "sulfonium S+",
            atom_of("S", 1, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "sulfur radical",
            atom_of("S", 0, Inferred, Radical::Doublet),
            vec![S, S, D],
            no,
        ),
        (
            "oxonium O+",
            atom_of("O", 1, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "silicon inferred",
            atom_of("Si", 0, Inferred, Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            // The valence model has no entry for silicon, so it cannot call the stated count
            // valid; a stated count is not taken at face value.
            "silicon stated 1 (unlisted element)",
            atom_of("Si", 0, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "carbon dication stated 1 (unlisted charge)",
            atom_of("C", 2, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "C stated 1 with an attachment point (unsupported by the model)",
            Atom {
                attachment_points: AttachmentPoints::new(&[1]).unwrap(),
                ..atom_of("C", 0, Specified(1), Radical::None)
            },
            vec![S, S, S],
            no,
        ),
        (
            "ammonium N+ stated 1 (a protonated amine is a centre)",
            atom_of("N", 1, Specified(1), Radical::None),
            vec![S, S, S],
            h,
        ),
        (
            "borohydride B- stated 1",
            atom_of("B", -1, Specified(1), Radical::None),
            vec![S, S, S],
            h,
        ),
        (
            "P stated 1 with three single bonds (valence 4, hypervalent)",
            atom_of("P", 0, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "S stated 1 with three single bonds (valence 4, hypervalent)",
            atom_of("S", 0, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "O+ stated 1 (valence 4, hypervalent)",
            atom_of("O", 1, Specified(1), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "carbocation stated 0",
            atom_of("C", 1, Specified(0), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "oxonium O+ stated 0",
            atom_of("O", 1, Specified(0), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "N- stated 0 (valence 3 where the model allows 2)",
            atom_of("N", -1, Specified(0), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "silicon stated 0",
            atom_of("Si", 0, Specified(0), Radical::None),
            vec![S, S, S],
            no,
        ),
        (
            "query atom",
            Atom::new(AtomKind::Query(QueryAtom::Any)),
            vec![S, S, S],
            no,
        ),
        (
            "C stated 1 with a double bond",
            atom_of("C", 0, Specified(1), Radical::None),
            vec![S, S, D],
            no,
        ),
        (
            "four neighbors, inferred",
            atom_of("C", 0, Inferred, Radical::None),
            vec![S, S, S, S],
            Ok(None),
        ),
        (
            "four neighbors, stated 0",
            atom_of("C", 0, Specified(0), Radical::None),
            vec![S, S, S, S],
            Ok(None),
        ),
        (
            "four neighbors, stated 1",
            atom_of("C", 0, Specified(1), Radical::None),
            vec![S, S, S, S],
            no,
        ),
        (
            "two neighbors",
            atom_of("C", 0, Inferred, Radical::None),
            vec![S, S],
            no,
        ),
        (
            "five neighbors",
            atom_of("C", 0, Inferred, Radical::None),
            vec![S, S, S, S, S],
            no,
        ),
    ];
    let mismatches: Vec<String> = rows
        .into_iter()
        .filter_map(|(name, atom, orders, expected)| {
            let actual = fourth_position(id(1), &atom, &orders);
            (actual != expected).then(|| format!("{name}: expected {expected:?}, got {actual:?}"))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "rows differ:\n{}",
        mismatches.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// Double bonds.
// ---------------------------------------------------------------------------------------------

/// E6: a1 (70, 115), a2 (100, 100) = a3 (130, 100), a4 (160, 85) (zig-zag) or (160, 115)
/// (U shape). Double bond b2 from a2 to a3. cross2(axis, ref) = 30 * 15 = 450 at a2's reference
/// and 30 * -15 = -450 at a3's (zig-zag, trans) or 450 (U shape, cis).
fn zigzag(u_shape: bool) -> Sketch {
    let mut sketch = Sketch::default();
    sketch
        .atom(1, "C", 70.0, 115.0)
        .atom(2, "C", 100.0, 100.0)
        .atom(3, "C", 130.0, 100.0)
        .atom(4, "C", 160.0, if u_shape { 115.0 } else { 85.0 })
        .single(1, 1, 2, None)
        .bond(2, 2, 3, BondOrder::Double, None)
        .single(3, 3, 4, None);
    sketch
}

fn relative(relation: Relation) -> DoubleBondState {
    DoubleBondState::Relative {
        begin_ref: slot(1),
        end_ref: slot(4),
        relation,
    }
}

#[test]
fn a_zigzag_double_bond_is_opposite_sides_and_a_u_shape_is_the_same_side() {
    assert_eq!(
        zigzag(false).double_bond(2),
        Ok(relative(Relation::OppositeSides))
    );
    assert_eq!(
        zigzag(true).double_bond(2),
        Ok(relative(Relation::SameSide))
    );
}

#[test]
fn a_double_bond_relation_survives_rotation_translation_and_reflection() {
    for u_shape in [false, true] {
        let sketch = zigzag(u_shape);
        let expected = sketch.double_bond(2);
        for degrees in 0..360 {
            assert_eq!(
                rotated(&sketch, f64::from(degrees)).double_bond(2),
                expected
            );
        }
        assert_eq!(
            reflected(&sketch).double_bond(2),
            expected,
            "cis/trans is mirror-invariant"
        );
        assert_eq!(
            sketch.moved(|x, y| (x + 50.0, y - 7.0)).double_bond(2),
            expected
        );
    }
}

#[test]
fn each_end_uses_its_lowest_identifier_neighbor_as_the_reference() {
    // a5 hangs above a2 (a higher id than a1, on the other side of the axis): the reference
    // stays a1, so the answer is unchanged. Choosing a5 would make it the same side.
    let mut sketch = zigzag(false);
    sketch.atom(5, "C", 100.0, 70.0).single(4, 2, 5, None);
    assert_eq!(sketch.double_bond(2), Ok(relative(Relation::OppositeSides)));
}

#[test]
fn a_query_atom_end_derives_like_any_other_end() {
    let mut sketch = zigzag(false);
    sketch.atom_with(2, Atom::new(AtomKind::Query(QueryAtom::Any)), 100.0, 100.0);
    assert_eq!(sketch.double_bond(2), Ok(relative(Relation::OppositeSides)));
}

#[test]
fn a_crossed_double_bond_is_unknown_and_reads_no_position() {
    let mut sketch = zigzag(false);
    sketch.bonds.get_mut(&bid(2)).unwrap().mark = Some(StereoMark::Crossed);
    sketch.positions.clear();
    assert_eq!(sketch.double_bond(2), Ok(DoubleBondState::Unknown));
}

#[test]
fn double_bond_refusals_are_structural_or_named() {
    let ends_without_reference = {
        let mut sketch = Sketch::default();
        sketch
            .atom(1, "C", 0.0, 0.0)
            .atom(2, "C", 30.0, 0.0)
            .atom(3, "C", 60.0, 10.0)
            .bond(1, 1, 2, BondOrder::Double, None)
            .single(2, 2, 3, None);
        sketch
    };
    assert_eq!(
        ends_without_reference.double_bond(1),
        Err(DeriveError::NotDerivable(NotDerivable::NoReference))
    );
    assert_eq!(
        zigzag(false).double_bond(1),
        Err(DeriveError::Structure(StereoError::DoubleBondRequired))
    );
    assert_eq!(
        zigzag(false).drawing().derive_double_bond(bid(9)),
        Err(DeriveError::UnknownBond(bid(9)))
    );
    // A cumulated double bond at an end.
    let mut cumulated = zigzag(false);
    cumulated
        .atom(5, "C", 190.0, 100.0)
        .bond(4, 3, 5, BondOrder::Double, None);
    assert_eq!(
        cumulated.double_bond(2),
        Err(DeriveError::Structure(StereoError::GeometryUnsupported))
    );
    // Three other neighbors at one end.
    let mut crowded = zigzag(false);
    crowded
        .atom(5, "C", 100.0, 130.0)
        .atom(6, "C", 100.0, 70.0)
        .atom(7, "C", 70.0, 85.0)
        .single(4, 2, 5, None)
        .single(5, 2, 6, None)
        .single(6, 2, 7, None);
    assert_eq!(
        crowded.double_bond(2),
        Err(DeriveError::Structure(StereoError::ReferenceInvalid))
    );
}

#[test]
fn a_double_bond_reference_on_the_axis_or_on_top_of_its_end_is_refused_at_the_limits() {
    let with_reference_on_axis = |sine: f64| {
        let mut sketch = zigzag(false);
        // a1 almost straight behind a2: direction (-cos t, sin t) from a2, sine of the angle
        // to the axis = sin t.
        let tilt = sine.asin();
        sketch.atom(1, "C", 100.0 - 30.0 * tilt.cos(), 100.0 - 30.0 * tilt.sin());
        sketch
    };
    assert_eq!(
        with_reference_on_axis(0.5 * COLLINEAR_SINE).double_bond(2),
        Err(DeriveError::NotDerivable(NotDerivable::Degenerate))
    );
    assert!(
        with_reference_on_axis(2.0 * COLLINEAR_SINE)
            .double_bond(2)
            .is_ok()
    );
    let mut on_top = zigzag(false);
    on_top.atom(1, "C", 100.0, 100.0 + 0.5 * MIN_BOND_LENGTH);
    assert_eq!(
        on_top.double_bond(2),
        Err(DeriveError::NotDerivable(NotDerivable::CoincidentAtoms))
    );
    let mut zero_axis = zigzag(false);
    zero_axis.atom(3, "C", 100.0, 100.0);
    assert_eq!(
        zero_axis.double_bond(2),
        Err(DeriveError::NotDerivable(NotDerivable::CoincidentAtoms))
    );
}

#[test]
fn two_substituents_on_the_same_side_of_an_end_still_derive_from_the_lowest_identifier() {
    let mut sketch = zigzag(false);
    // a5 on the same side of the axis as a1, higher id.
    sketch.atom(5, "C", 80.0, 130.0).single(4, 2, 5, None);
    assert_eq!(sketch.double_bond(2), Ok(relative(Relation::OppositeSides)));
}

// ---------------------------------------------------------------------------------------------
// Derived states are storable: the document accepts each one as it is.
// ---------------------------------------------------------------------------------------------

fn slot_ref(slot: Slot) -> SlotRef {
    match slot {
        Slot::Atom(atom) => SlotRef::Atom(AtomRef::Id(atom)),
        Slot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
        Slot::LonePair => SlotRef::LonePair,
    }
}

/// A document holding the sketch's atoms, bonds, marks and positions, in id order. The sketch's
/// ids must be 1, 2, 3, ... with no gaps.
fn document_of(sketch: &Sketch) -> Document {
    let mut document = Document::new(DocumentId::parse("doc-derive").unwrap(), "Derive").unwrap();
    let mut commands: Vec<Command> = sketch
        .atoms
        .iter()
        .map(|(atom_id, atom)| Command::AddAtom {
            atom: atom.clone(),
            positions: NewAtomPositions::Single(sketch.positions[atom_id]),
            bond_to: None,
        })
        .collect();
    for bond in sketch.bonds.values() {
        commands.push(Command::AddBond {
            begin: AtomRef::Id(bond.begin),
            end: AtomRef::Id(bond.end),
            order: bond.order.clone(),
        });
    }
    for (bond_id, bond) in &sketch.bonds {
        if bond.mark.is_some() {
            commands.push(Command::SetBondMark {
                bond: BondRef::Id(*bond_id),
                mark: bond.mark,
            });
        }
    }
    document
        .apply_reporting(&commands, &TransactionSelection::default())
        .unwrap_or_else(|error| panic!("{error}"));
    document
}

fn store(document: &mut Document, center: u32, state: CenterState) {
    let configuration = match state {
        CenterState::Tetrahedral { slots, turn } => AtomConfiguration::Tetrahedral {
            neighbors: slots.map(slot_ref),
            turn,
        },
        CenterState::Unknown { slots } => AtomConfiguration::Unknown {
            neighbors: slots.map(slot_ref),
        },
    };
    document
        .apply_reporting(
            &[Command::SetAtomConfiguration {
                atom: AtomRef::Id(id(center)),
                configuration: Some(configuration),
            }],
            &TransactionSelection::default(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        document.stereo().get(StereoKey::Center(id(center))),
        Some(&StereoState::Center(state)),
        "the stored record is the derived state"
    );
}

#[test]
fn derived_centers_are_accepted_and_stored_unchanged_by_a_document() {
    let h = Slot::ImplicitHydrogen;
    let cases: Vec<Sketch> = vec![
        e1(),
        e2_with(3, 2, 4, 5, false),
        {
            let mut sulfoxide = Sketch::default();
            sulfoxide
                .atom(1, "S", 100.0, 100.0)
                .atom(2, "O", 100.0, 70.0)
                .atom(3, "C", 126.0, 115.0)
                .atom(4, "C", 74.0, 115.0)
                .bond(1, 1, 2, BondOrder::Double, None)
                .single(2, 1, 3, wedge(BondEnd::Begin))
                .single(3, 1, 4, None);
            sulfoxide
        },
        {
            let mut wavy = e1();
            wavy.single(2, 1, 3, Some(StereoMark::Wavy));
            wavy
        },
    ];
    for sketch in cases {
        let mut document = document_of(&sketch);
        let derived = Drawing::of_document(&document, document.first_instance())
            .derive_center(id(1))
            .unwrap();
        assert_eq!(
            derived,
            sketch.center(1).unwrap(),
            "document path equals map path"
        );
        store(&mut document, 1, derived);
    }
    // The sulfoxide's fourth position is a lone pair, not a hydrogen.
    let mut sulfoxide = Sketch::default();
    sulfoxide
        .atom(1, "S", 100.0, 100.0)
        .atom(2, "O", 100.0, 70.0)
        .atom(3, "C", 126.0, 115.0)
        .atom(4, "C", 74.0, 115.0)
        .bond(1, 1, 2, BondOrder::Double, None)
        .single(2, 1, 3, wedge(BondEnd::Begin))
        .single(3, 1, 4, None);
    let state = sulfoxide.center(1).unwrap();
    assert_eq!(state.slots()[3], Slot::LonePair);
    assert_ne!(state.slots()[3], h);
}

#[test]
fn derived_double_bonds_are_accepted_and_stored_unchanged_by_a_document() {
    for u_shape in [false, true] {
        let sketch = zigzag(u_shape);
        let mut document = document_of(&sketch);
        let derived = Drawing::of_document(&document, document.first_instance())
            .derive_double_bond(bid(2))
            .unwrap();
        let DoubleBondState::Relative {
            begin_ref,
            end_ref,
            relation,
        } = derived
        else {
            panic!("a relative state");
        };
        document
            .apply_reporting(
                &[Command::SetBondConfiguration {
                    bond: BondRef::Id(bid(2)),
                    configuration: Some(BondConfiguration::Relative {
                        begin_neighbor: slot_ref(begin_ref),
                        end_neighbor: slot_ref(end_ref),
                        relation,
                    }),
                }],
                &TransactionSelection::default(),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            document.stereo().get(StereoKey::DoubleBond(bid(2))),
            Some(&StereoState::DoubleBond(derived))
        );
    }
}

/// Derives crossed double bond `b1` of `sketch` and stores the result through a document,
/// the way the stored-state tests above do.
fn derive_and_store_crossed(sketch: &Sketch) -> Result<DoubleBondState, DeriveError> {
    let mut document = document_of(sketch);
    let derived =
        Drawing::of_document(&document, document.first_instance()).derive_double_bond(bid(1))?;
    assert_eq!(
        derived,
        sketch.double_bond(1).unwrap(),
        "document path equals map path"
    );
    document
        .apply_reporting(
            &[Command::SetBondConfiguration {
                bond: BondRef::Id(bid(1)),
                configuration: Some(BondConfiguration::Unknown),
            }],
            &TransactionSelection::default(),
        )
        .unwrap_or_else(|error| panic!("the model refused the derived state {derived:?}: {error}"));
    assert_eq!(
        document.stereo().get(StereoKey::DoubleBond(bid(1))),
        Some(&StereoState::DoubleBond(derived)),
        "the stored record is the derived state"
    );
    Ok(derived)
}

#[test]
fn a_crossed_terminal_or_half_terminal_double_bond_is_unknown_and_the_document_stores_it() {
    // C=O with no other neighbor at either end.
    let mut terminal = Sketch::default();
    terminal
        .atom(1, "C", 0.0, 0.0)
        .atom(2, "O", 30.0, 0.0)
        .bond(1, 1, 2, BondOrder::Double, Some(StereoMark::Crossed));
    assert_eq!(
        derive_and_store_crossed(&terminal),
        Ok(DoubleBondState::Unknown)
    );
    // One end has a neighbor, the other none: an uncrossed bond would be NoReference.
    let mut half = Sketch::default();
    half.atom(1, "C", 0.0, 0.0)
        .atom(2, "N", 30.0, 0.0)
        .atom(3, "C", -15.0, 20.0)
        .bond(1, 1, 2, BondOrder::Double, Some(StereoMark::Crossed))
        .single(2, 1, 3, None);
    assert_eq!(
        derive_and_store_crossed(&half),
        Ok(DoubleBondState::Unknown)
    );
    let mut uncrossed = half.marked(|mark| mark);
    uncrossed.bonds.get_mut(&bid(1)).unwrap().mark = None;
    assert_eq!(
        uncrossed.double_bond(1),
        Err(DeriveError::NotDerivable(NotDerivable::NoReference))
    );
}

#[test]
fn a_crossed_double_bond_beside_a_cumulated_bond_is_refused_as_the_document_refuses_it() {
    // An allene C1=C2=C3 whose first double bond is crossed: derive never returns a state
    // the model cannot hold, so it reports the model's own refusal.
    let mut allene = Sketch::default();
    allene
        .atom(1, "C", 0.0, 0.0)
        .atom(2, "C", 30.0, 0.0)
        .atom(3, "C", 60.0, 0.0)
        .bond(1, 1, 2, BondOrder::Double, Some(StereoMark::Crossed))
        .bond(2, 2, 3, BondOrder::Double, None);
    let refusal = DeriveError::Structure(StereoError::GeometryUnsupported);
    assert_eq!(allene.double_bond(1), Err(refusal));
    let mut document = document_of(&allene);
    assert_eq!(derive_and_store_crossed(&allene), Err(refusal));
    let stored = document.apply_reporting(
        &[Command::SetBondConfiguration {
            bond: BondRef::Id(bid(1)),
            configuration: Some(BondConfiguration::Unknown),
        }],
        &TransactionSelection::default(),
    );
    assert_eq!(
        stored.map_err(|error| error.error),
        Err(EditError::Stereo(StereoError::GeometryUnsupported)),
        "the document refuses the same state for the same reason"
    );
}

#[test]
fn an_unknown_instance_has_no_positions() {
    let document = document_of(&e1());
    let other = chemifree_domain::document::ObjectId::from_serial(999).unwrap();
    assert!(matches!(
        Drawing::of_document(&document, other).derive_center(id(1)),
        Err(DeriveError::MissingPosition(_))
    ));
}

// ---------------------------------------------------------------------------------------------
// Transform laws on stored values.
// ---------------------------------------------------------------------------------------------

#[test]
fn flipping_a_mark_over_swaps_wedge_and_hash_and_leaves_the_rest() {
    for narrow in [BondEnd::Begin, BondEnd::End] {
        assert_eq!(
            StereoMark::Wedge { narrow }.flipped_over(),
            StereoMark::Hash { narrow }
        );
        assert_eq!(
            StereoMark::Hash { narrow }.flipped_over(),
            StereoMark::Wedge { narrow }
        );
        for mark in [StereoMark::Wedge { narrow }, StereoMark::Hash { narrow }] {
            assert_eq!(mark.flipped_over().flipped_over(), mark, "an involution");
        }
    }
    assert_eq!(StereoMark::Wavy.flipped_over(), StereoMark::Wavy);
    assert_eq!(StereoMark::Crossed.flipped_over(), StereoMark::Crossed);
}

#[test]
fn mirroring_turns_a_tetrahedral_center_and_keeps_an_unknown_one() {
    let clockwise = tetra(e1_slots(), Turn::Clockwise);
    let anticlockwise = tetra(e1_slots(), Turn::Anticlockwise);
    assert_eq!(clockwise.mirrored(), anticlockwise);
    assert_eq!(anticlockwise.mirrored(), clockwise);
    let unknown = CenterState::unknown(e1_slots()).unwrap();
    assert_eq!(unknown.mirrored(), unknown);
}

#[test]
fn mirroring_a_table_turns_centers_and_keeps_double_bonds_and_unknowns() {
    // A document with a center (a1..a4), an unknown center (a5..a8) and a double bond.
    let mut sketch = e1();
    sketch
        .atom(5, "C", 300.0, 100.0)
        .atom(6, "F", 300.0, 70.0)
        .atom(7, "Cl", 326.0, 115.0)
        .atom(8, "Br", 274.0, 115.0)
        .single(4, 5, 6, None)
        .single(5, 5, 7, None)
        .single(6, 5, 8, None)
        .atom(9, "C", 400.0, 100.0)
        .atom(10, "C", 430.0, 100.0)
        .atom(11, "C", 370.0, 115.0)
        .atom(12, "C", 460.0, 85.0)
        .bond(7, 9, 10, BondOrder::Double, None)
        .single(8, 9, 11, None)
        .single(9, 10, 12, None);
    let mut document = document_of(&sketch);
    store(&mut document, 1, sketch.center(1).unwrap());
    store(
        &mut document,
        5,
        CenterState::unknown(quad_with_h(6, 7, 8)).unwrap(),
    );
    document
        .apply_reporting(
            &[Command::SetBondConfiguration {
                bond: BondRef::Id(bid(7)),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: SlotRef::Atom(AtomRef::Id(id(11))),
                    end_neighbor: SlotRef::Atom(AtomRef::Id(id(12))),
                    relation: Relation::OppositeSides,
                }),
            }],
            &TransactionSelection::default(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let table = document.stereo();
    let mirrored = table.mirrored();
    assert_eq!(mirrored.len(), table.len());
    assert_eq!(
        mirrored.get(StereoKey::Center(id(1))),
        Some(&StereoState::Center(tetra(e1_slots(), Turn::Anticlockwise)))
    );
    assert_eq!(
        mirrored.get(StereoKey::Center(id(5))),
        table.get(StereoKey::Center(id(5)))
    );
    assert_eq!(
        mirrored.get(StereoKey::DoubleBond(bid(7))),
        table.get(StereoKey::DoubleBond(bid(7)))
    );
    assert_ne!(&mirrored, table);
    assert_eq!(&mirrored.mirrored(), table, "an involution");
}

fn quad_with_h(a: u32, b: u32, c: u32) -> [Slot; 4] {
    [slot(a), slot(b), slot(c), Slot::ImplicitHydrogen]
}

#[test]
fn the_turn_over_any_order_of_the_slots_flips_with_the_parity_of_the_order() {
    for non_atom in [Slot::ImplicitHydrogen, Slot::LonePair] {
        let canonical = [slot(2), slot(3), slot(4), non_atom];
        for turn in [Turn::Anticlockwise, Turn::Clockwise] {
            let state = tetra(canonical, turn);
            for order in permutations_of_four() {
                let given = order.map(|index| canonical[index]);
                let expected = if inversions(&order) % 2 == 1 {
                    turn.flipped()
                } else {
                    turn
                };
                assert_eq!(
                    state.turn_for_order(given),
                    Ok(Some(expected)),
                    "{given:?} of {turn:?}"
                );
                // Stating that turn over that order stores the same record.
                assert_eq!(CenterState::tetrahedral(given, expected), Ok(state));
            }
        }
    }
}

#[test]
fn an_unknown_center_has_no_turn_and_a_foreign_order_is_refused() {
    let unknown = CenterState::unknown(e1_slots()).unwrap();
    assert_eq!(unknown.turn_for_order(e1_slots()), Ok(None));
    let state = tetra(e1_slots(), Turn::Clockwise);
    let refusal = Err(StereoError::CenterNeighbors);
    // A repeated slot, a foreign atom, and the other label of the non-atom slot.
    assert_eq!(
        state.turn_for_order([slot(2), slot(2), slot(4), Slot::ImplicitHydrogen]),
        refusal
    );
    assert_eq!(
        state.turn_for_order([slot(2), slot(3), slot(9), Slot::ImplicitHydrogen]),
        refusal
    );
    assert_eq!(
        state.turn_for_order([slot(2), slot(3), slot(4), Slot::LonePair]),
        refusal
    );
}

// ---------------------------------------------------------------------------------------------
// Bounded work and reasons.
// ---------------------------------------------------------------------------------------------

#[test]
fn deriving_every_atom_of_a_document_sized_graph_builds_one_index() {
    // 2,000 atoms and about 4,000 bonds, every bond a hash from its begin atom. One Drawing
    // serves every call; each call touches only the atom's neighbors.
    let mut sketch = Sketch::default();
    for serial in 1..=2_000_u32 {
        let angle = f64::from(serial) * 0.1;
        sketch.atom(serial, "C", 1000.0 * angle.cos(), 1000.0 * angle.sin());
    }
    let mut bond_serial = 0;
    for serial in 1..=1_998_u32 {
        for step in [1, 2] {
            bond_serial += 1;
            sketch.single(bond_serial, serial, serial + step, hash(BondEnd::Begin));
        }
    }
    assert!(sketch.bonds.len() <= 4_000);
    let drawing = sketch.drawing();
    let mut derived = 0;
    for serial in 1..=2_000_u32 {
        match drawing.derive_center(id(serial)) {
            Ok(_) => derived += 1,
            Err(DeriveError::Structure(_) | DeriveError::NotDerivable(_)) => {}
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
    assert!(derived > 0, "some interior atoms are centers");
}

#[test]
fn the_reasons_have_distinct_codes_with_the_shared_prefix() {
    let mut codes: Vec<&str> = NotDerivable::ALL
        .iter()
        .map(|reason| reason.code())
        .collect();
    assert!(
        codes
            .iter()
            .all(|code| code.starts_with("stereo_not_derivable_"))
    );
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), NotDerivable::ALL.len());
    assert!(!codes.contains(&"stereo_not_derivable_query_atom"));
}
