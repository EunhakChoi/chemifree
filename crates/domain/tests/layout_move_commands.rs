//! The edit that applies a layout preview. `layout::move_commands` turns each moved layout atom
//! into one `MoveAtoms` command with its exact delta, and applying those commands puts every atom
//! at its placed position. The expected commands are written out by hand from the placements of
//! each case, so a wrong delta, order, instance or count fails with the atom named.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, MAX_COMMANDS,
    NewAtomPositions, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{self, Placement, Preview, Scope};
use chemifree_domain::scene::Selection;

/// The result of a test or helper, so that a failed setup returns its error instead of panicking.
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The tolerance, in pt, between a committed position and the placed one.
const COMMIT_TOLERANCE_PT: f64 = 1e-9;

fn atom(serial: u32) -> TestResult<AtomId> {
    AtomId::from_serial(serial).ok_or_else(|| format!("atom serial {serial} is not valid").into())
}

fn layout_instance() -> TestResult<ObjectId> {
    ObjectId::from_serial(1).ok_or_else(|| "object serial 1 is not valid".into())
}

fn point(x: f64, y: f64) -> TestResult<Point> {
    Ok(Point::new(x, y)?)
}

/// A placement of atom `serial` from `before` to `after`, both (x, y) in pt.
fn placement(serial: u32, before: (f64, f64), after: (f64, f64)) -> TestResult<Placement> {
    Ok(Placement {
        atom: atom(serial)?,
        before: point(before.0, before.1)?,
        after: point(after.0, after.1)?,
    })
}

/// A preview that holds `placements` and no components or overlaps.
fn hand_built_preview(placements: Vec<Placement>) -> Preview {
    Preview {
        components: Vec::new(),
        positions: placements,
        overlap_count: 0,
        overlaps: Vec::new(),
    }
}

/// The command that moves atom `serial` by `delta` (dx, dy) in `instance`.
fn move_atom(serial: u32, delta: (f64, f64), instance: ObjectId) -> TestResult<Command> {
    Ok(Command::MoveAtoms {
        atoms: vec![atom(serial)?],
        delta: Vector {
            dx: delta.0,
            dy: delta.1,
        },
        instance: Some(instance),
    })
}

/// The `MoveAtoms` deltas of `commands` as bit patterns, in order, so that `-0.0` and `0.0` are
/// told apart.
fn delta_bits(commands: &[Command]) -> Vec<(u64, u64)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::MoveAtoms { delta, .. } => Some((delta.dx.to_bits(), delta.dy.to_bits())),
            _ => None,
        })
        .collect()
}

/// Whether `committed` lies within `COMMIT_TOLERANCE_PT` of `placed` on both axes.
fn within_commit_tolerance(committed: Point, placed: Point) -> bool {
    (committed.x() - placed.x()).abs() <= COMMIT_TOLERANCE_PT
        && (committed.y() - placed.y()).abs() <= COMMIT_TOLERANCE_PT
}

/// A carbon at (x, y), bonded to the carbon that command `bonded_to` of the same transaction adds.
fn add_carbon(x: f64, y: f64, bonded_to: Option<usize>) -> TestResult<Command> {
    Ok(Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(point(x, y)?),
        bond_to: bonded_to.map(|index| BondTo {
            atom: AtomRef::Created(index),
            order: BondOrder::Single,
        }),
    })
}

/// Carbons at `places`, each bonded to the carbon at the index given with its place.
fn carbons(places: &[(f64, f64, Option<usize>)]) -> TestResult<Document> {
    let id = DocumentId::parse("layout").ok_or("layout is a valid document id")?;
    let mut document = Document::new(id, "Layout")?;
    let commands = places
        .iter()
        .map(|&(x, y, bonded_to)| add_carbon(x, y, bonded_to))
        .collect::<TestResult<Vec<Command>>>()?;
    document.apply(&commands)?;
    Ok(document)
}

/// The butane of atoms 1 to 4 in a row 30 pt apart, bonds 1 to 3.
fn butane_row() -> TestResult<Document> {
    carbons(&[
        (0.0, 0.0, None),
        (30.0, 0.0, Some(0)),
        (60.0, 0.0, Some(1)),
        (90.0, 0.0, Some(2)),
    ])
}

/// The selection of the atoms with the given serials.
fn selection(serials: &[u32]) -> TestResult<Selection> {
    let atoms = serials
        .iter()
        .map(|&serial| atom(serial))
        .collect::<TestResult<BTreeSet<AtomId>>>()?;
    Ok(Selection {
        atoms,
        ..Selection::default()
    })
}

/// Carbons 1 to `count` in a row 30 pt apart, each bonded to the one before. Each transaction
/// holds at most 250 carbons, below the command limit.
fn long_chain(count: usize) -> TestResult<Document> {
    const CHUNK: usize = 250;
    let id = DocumentId::parse("chain").ok_or("chain is a valid document id")?;
    let mut document = Document::new(id, "Chain")?;
    for start in (0..count).step_by(CHUNK) {
        let end = (start + CHUNK).min(count);
        // The last carbon added before this transaction, which its first carbon bonds to. Atom
        // identifiers grow with each addition, so it is the largest one present.
        let last = document.atoms().map(|(id, _)| id).max();
        let mut commands = Vec::with_capacity(end - start);
        for index in start..end {
            let bond_to = if index == start {
                last.map(|previous| BondTo {
                    atom: AtomRef::Id(previous),
                    order: BondOrder::Single,
                })
            } else {
                Some(BondTo {
                    atom: AtomRef::Created(index - start - 1),
                    order: BondOrder::Single,
                })
            };
            let x = f64::from(u32::try_from(index)?) * 30.0;
            commands.push(Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(point(x, 0.0)?),
                bond_to,
            });
        }
        document.apply(&commands)?;
    }
    Ok(document)
}

#[test]
fn an_atom_at_its_place_gets_no_command() -> TestResult {
    let preview = hand_built_preview(vec![
        placement(1, (10.0, 20.0), (10.0, 20.0))?,
        placement(2, (0.0, 5.0), (-0.0, 5.0))?,
    ]);
    let commands = layout::move_commands(&preview, layout_instance()?);
    assert!(
        commands.is_empty(),
        "an atom at its place must get no command, got {commands:?}"
    );
    Ok(())
}

#[test]
fn only_moves_beyond_the_move_tolerance_get_a_command() -> TestResult {
    let instance = layout_instance()?;
    let preview = hand_built_preview(vec![
        // Within the tolerance on both axes: the rounding of a committed move, not a move.
        placement(1, (0.0, 0.0), (1e-10, 0.0))?,
        // Beyond the tolerance on x.
        placement(2, (0.0, 0.0), (1e-6, 0.0))?,
        // Within the tolerance on x but beyond it on y, so the atom moves by both differences.
        placement(3, (0.0, 0.0), (1e-10, 1e-6))?,
        // Exactly at the tolerance, which still counts as within it.
        placement(4, (0.0, 0.0), (layout::MOVE_TOLERANCE_PT, 0.0))?,
    ]);
    let commands = layout::move_commands(&preview, instance);
    let expected = vec![
        move_atom(2, (1e-6, 0.0), instance)?,
        move_atom(3, (1e-10, 1e-6), instance)?,
    ];
    assert_eq!(
        commands, expected,
        "an atom within MOVE_TOLERANCE_PT on both axes gets no command, one beyond it gets one"
    );
    Ok(())
}

#[test]
fn one_command_per_moved_atom_ascending_with_its_exact_delta_and_instance() -> TestResult {
    let instance = layout_instance()?;
    let preview = hand_built_preview(vec![
        placement(1, (0.0, 0.0), (6.5, 7.25))?,
        placement(2, (30.0, 0.0), (30.0, 0.0))?,
        placement(3, (60.0, 0.0), (57.99, 7.5))?,
        placement(4, (90.0, 0.0), (96.5, 7.25))?,
    ]);
    let commands = layout::move_commands(&preview, instance);
    let expected = vec![
        move_atom(1, (6.5 - 0.0, 7.25 - 0.0), instance)?,
        move_atom(3, (57.99 - 60.0, 7.5 - 0.0), instance)?,
        move_atom(4, (96.5 - 90.0, 7.25 - 0.0), instance)?,
    ];
    assert_eq!(
        commands, expected,
        "one command per moved atom, ascending, each naming the instance"
    );
    assert_eq!(
        delta_bits(&commands),
        delta_bits(&expected),
        "each delta must be after minus before, computed in binary64"
    );
    Ok(())
}

#[test]
fn applying_the_commands_puts_each_atom_at_its_placed_position() -> TestResult {
    let mut document = butane_row()?;
    let prepared = layout::prepare(
        &document,
        document.first_instance(),
        &selection(&[2])?,
        Scope::Components,
    )?;
    let preview = layout::place(&prepared)?;
    assert_eq!(preview.positions.len(), 4, "the whole butane is laid out");
    let commands = layout::move_commands(&preview, prepared.instance);
    assert!(
        !commands.is_empty(),
        "a straight row of four atoms is re-laid out, so some atom moves"
    );

    let depth = document.undo_depth();
    document.apply(&commands)?;
    assert_eq!(
        document.undo_depth(),
        depth + 1,
        "the commands apply as one transaction"
    );
    for placement in &preview.positions {
        let committed = document
            .position(prepared.instance, placement.atom)
            .ok_or("an applied atom has no position")?;
        assert!(
            within_commit_tolerance(committed, placement.after),
            "atom {:?} is committed at {committed:?} but placed at {:?}",
            placement.atom,
            placement.after
        );
    }
    Ok(())
}

#[test]
fn a_500_atom_chain_needs_at_most_max_commands() -> TestResult {
    let mut document = long_chain(500)?;
    let prepared = layout::prepare(
        &document,
        document.first_instance(),
        &selection(&[1])?,
        Scope::Components,
    )?;
    let preview = layout::place(&prepared)?;
    assert_eq!(preview.positions.len(), 500, "the whole chain is laid out");
    let commands = layout::move_commands(&preview, prepared.instance);
    assert!(
        commands.len() <= MAX_COMMANDS,
        "{} atoms need {} commands, above the {MAX_COMMANDS} a transaction holds",
        preview.positions.len(),
        commands.len()
    );

    let depth = document.undo_depth();
    document.apply(&commands)?;
    assert_eq!(
        document.undo_depth(),
        depth + 1,
        "the commands apply as one transaction"
    );
    Ok(())
}
