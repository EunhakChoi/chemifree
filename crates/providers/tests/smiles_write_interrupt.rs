//! Stopping a SMILES write. `write_interruptible` gives exactly what `write_with_groups` gives
//! when nothing stops it. A stop at any question the writer asks is `Err(Interrupted)`, never a
//! refusal, a loss or a partial text. Stops are placed by counting questions, never by time.

use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::smiles::{
    SmilesCode, SmilesCodec, SmilesError, SmilesSlot, SmilesStyle, SmilesWrite,
};
use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::document::{
    AtomId, AtomRef, BondRef, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::{AtomConfiguration, BondConfiguration, SlotRef};
use chemifree_domain::stereo_group::StereoGroupTable;
use chemifree_providers::OpenSmilesCodec;

/// What `write_interruptible` gives: the writer's own result, or the stop.
type Outcome = Result<Result<SmilesWrite, SmilesError>, Interrupted>;

const STYLES: [SmilesStyle; 2] = [SmilesStyle::Recorded, SmilesStyle::Canonical];

/// Counts the questions a write asks, numbered from 1, and stops at the `question`-th one with
/// `reason` when there is one.
struct CancelAt {
    question: Option<usize>,
    reason: Interrupted,
    asked: AtomicUsize,
}

impl CancelAt {
    /// Counts the questions and never stops.
    fn counting() -> Self {
        Self {
            question: None,
            reason: Interrupted::Cancelled,
            asked: AtomicUsize::new(0),
        }
    }

    /// Stops at the `question`-th question with `reason`.
    fn at(question: usize, reason: Interrupted) -> Self {
        Self {
            question: Some(question),
            reason,
            asked: AtomicUsize::new(0),
        }
    }

    /// The number of questions asked so far.
    fn asked(&self) -> usize {
        self.asked.load(Ordering::Relaxed)
    }
}

impl Interrupt for CancelAt {
    fn check(&self) -> Result<(), Interrupted> {
        let asked = self.asked.fetch_add(1, Ordering::Relaxed) + 1;
        if self.question == Some(asked) {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

fn atom(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

fn empty_document() -> Document {
    Document::new(
        DocumentId::parse("smiles-write-interrupt").unwrap(),
        "Interrupt",
    )
    .unwrap()
}

/// The drawing a SMILES text states. Atom `n` of the text is the drawing's atom `n + 1`, and the
/// bonds are added in text order after the atoms. With `with_stereo`, the drawing also keeps the
/// centers, directional bonds and enhanced groups that the text states.
fn drawing(text: &str, with_stereo: bool) -> Document {
    let read = OpenSmilesCodec
        .read(text)
        .unwrap_or_else(|error| panic!("{text}: {error:?}"));
    let atoms = read.atoms.len();
    let mut commands: Vec<Command> = read
        .atoms
        .iter()
        .map(|entry| Command::AddAtom {
            atom: entry.atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(read.bonds.iter().map(|bond| Command::AddBond {
        begin: AtomRef::Id(atom(bond.begin)),
        end: AtomRef::Id(atom(bond.end)),
        order: bond.order.clone(),
    }));
    if with_stereo {
        commands.extend(read.stereo.double_bonds.iter().map(|found| {
            Command::SetBondConfiguration {
                bond: BondRef::Created(atoms + found.bond),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: SlotRef::Atom(AtomRef::Id(atom(found.begin_neighbor))),
                    end_neighbor: SlotRef::Atom(AtomRef::Id(atom(found.end_neighbor))),
                    relation: found.relation,
                }),
            }
        }));
        commands.extend(
            read.stereo
                .centers
                .iter()
                .map(|center| Command::SetAtomConfiguration {
                    atom: AtomRef::Id(atom(center.atom)),
                    configuration: Some(AtomConfiguration::Tetrahedral {
                        neighbors: center.slots.map(|slot| match slot {
                            SmilesSlot::Atom(index) => SlotRef::Atom(AtomRef::Id(atom(index))),
                            SmilesSlot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
                        }),
                        turn: center.turn,
                    }),
                }),
        );
        commands.extend(read.stereo.groups.iter().map(|group| {
            Command::CreateStereoGroup {
                kind: group.kind,
                number: group.number,
                atoms: group
                    .atoms
                    .iter()
                    .map(|&index| AtomRef::Id(atom(index)))
                    .collect(),
            }
        }));
    }
    let mut document = empty_document();
    document.apply(&commands).unwrap();
    document
}

/// A drawing as the writer receives it: its graph and the stereo groups kept with it.
struct Input {
    graph: MoleculeGraph,
    groups: StereoGroupTable,
}

impl Input {
    fn of(document: &Document) -> Self {
        Self {
            graph: MoleculeGraph::from_document(document),
            groups: document.stereo_groups().clone(),
        }
    }
}

/// The drawings every case runs over. Each stands for a path of the writer: the aromaticity
/// model, the stereo stages, a refusal that only the canonical form makes (before its first
/// question for `[CH2]`, after one for the ring radical) and the empty graph.
fn corpus() -> Vec<(&'static str, Input)> {
    vec![
        ("Kekulé benzene", Input::of(&drawing("C1=CC=CC=C1", true))),
        ("aromatic benzene", Input::of(&drawing("c1ccccc1", true))),
        ("naphthalene", Input::of(&drawing("c1ccc2ccccc2c1", true))),
        (
            "center with an enhanced group",
            Input::of(&drawing("C[C@H](N)O |&1:1|", true)),
        ),
        (
            "directional double bond",
            Input::of(&drawing("F/C=C/F", true)),
        ),
        (
            "radical outside the canonical model",
            Input::of(&drawing("[CH2]", true)),
        ),
        (
            "ring radical outside the canonical model",
            Input::of(&drawing("[CH]1CCCCC1", true)),
        ),
        ("empty graph", Input::of(&empty_document())),
    ]
}

/// The write of `input` in `style`, with `interrupt` asked at the writer's checkpoints.
fn interruptible(input: &Input, style: SmilesStyle, interrupt: &dyn Interrupt) -> Outcome {
    OpenSmilesCodec.write_interruptible(&input.graph, style, &input.groups, interrupt)
}

/// The write of `input` in `style` through `write_with_groups`: the result a stoppable write
/// must give when nothing stops it.
fn plain(input: &Input, style: SmilesStyle) -> Result<SmilesWrite, SmilesError> {
    OpenSmilesCodec.write_with_groups(&input.graph, style, &input.groups)
}

/// The outcome of the write of `input` in `style` under a counting interrupt that never stops
/// it, with the number of questions it asked.
fn counted(input: &Input, style: SmilesStyle) -> (Outcome, usize) {
    let counter = CancelAt::counting();
    let outcome = interruptible(input, style, &counter);
    (outcome, counter.asked())
}

#[test]
fn an_interrupt_that_never_stops_gives_the_write_with_groups_result() {
    for (name, input) in corpus() {
        for style in STYLES {
            let expected: Outcome = Ok(plain(&input, style));
            assert_eq!(
                interruptible(&input, style, &NEVER),
                expected,
                "{name} in {style:?}: NEVER must give the write_with_groups result"
            );
            let (counted_outcome, _) = counted(&input, style);
            assert_eq!(
                counted_outcome, expected,
                "{name} in {style:?}: an interrupt that counts and never stops must give the write_with_groups result"
            );
        }
    }
}

#[test]
fn a_stop_at_any_question_is_a_stop_and_a_later_stop_changes_nothing() {
    for (name, input) in corpus() {
        for style in STYLES {
            let (uninterrupted, total) = counted(&input, style);
            if matches!(uninterrupted, Ok(Ok(_))) {
                assert!(
                    total >= 1,
                    "{name} in {style:?} is written, so the writer must ask at least one question, asked {total}"
                );
            }
            for question in 1..=total {
                assert_eq!(
                    interruptible(
                        &input,
                        style,
                        &CancelAt::at(question, Interrupted::TimeLimit)
                    ),
                    Err(Interrupted::TimeLimit),
                    "{name} in {style:?}: a time limit at question {question} of {total} must stop the write, not give a refusal, a loss or a partial text"
                );
                assert_eq!(
                    interruptible(
                        &input,
                        style,
                        &CancelAt::at(question, Interrupted::Cancelled)
                    ),
                    Err(Interrupted::Cancelled),
                    "{name} in {style:?}: a cancel at question {question} of {total} must stop the write"
                );
            }
            assert_eq!(
                interruptible(
                    &input,
                    style,
                    &CancelAt::at(total + 1, Interrupted::TimeLimit)
                ),
                uninterrupted,
                "{name} in {style:?}: a stop after the last of {total} questions must change nothing"
            );
        }
    }
}

#[test]
fn the_unstopped_write_gives_the_texts_and_refusals_that_the_codec_gives() {
    for (text, style, expected) in [
        ("C1=CC=CC=C1", SmilesStyle::Recorded, "C1=CC=CC=C1"),
        ("c1ccccc1", SmilesStyle::Recorded, "c1ccccc1"),
        ("C1=CC=CC=C1", SmilesStyle::Canonical, "c1ccccc1"),
        ("[CH3]", SmilesStyle::Canonical, "[CH3]"),
        ("F/C=C/F", SmilesStyle::Recorded, "F/C=C/F"),
        (
            "C[C@H](N)O |&1:1|",
            SmilesStyle::Recorded,
            "C[C@H](N)O |&1:1|",
        ),
    ] {
        let input = Input::of(&drawing(text, true));
        let written = interruptible(&input, style, &NEVER)
            .unwrap_or_else(|stop| panic!("{text} in {style:?}: NEVER gave {stop:?}"))
            .unwrap_or_else(|error| panic!("{text} in {style:?} is refused: {error:?}"));
        assert_eq!(written.text, expected, "{text} in {style:?}");
    }
    for text in ["[CH2]", "[CH]1CCCCC1"] {
        let radical = Input::of(&drawing(text, true));
        let refused = interruptible(&radical, SmilesStyle::Canonical, &NEVER)
            .expect("NEVER cannot stop the write")
            .expect_err("a radical outside the canonical model is refused");
        assert_eq!(refused.code, SmilesCode::CanonicalUnsupported, "{text}");
    }
    let empty = Input::of(&empty_document());
    for style in STYLES {
        let refused = interruptible(&empty, style, &NEVER)
            .expect("NEVER cannot stop the write")
            .expect_err("an empty graph is refused");
        assert_eq!(refused.code, SmilesCode::EmptyGraph, "{style:?}");
    }
}

#[test]
fn the_kekule_benzene_write_asks_questions_in_both_styles() {
    let benzene = Input::of(&drawing("C1=CC=CC=C1", true));
    for style in STYLES {
        let (_, asked) = counted(&benzene, style);
        assert!(
            asked >= 1,
            "Kekulé benzene in {style:?} must ask at least one question, asked {asked}"
        );
    }
}

#[test]
fn the_aromaticity_model_asks_questions_in_the_recorded_style() {
    let (_, kekule) = counted(
        &Input::of(&drawing("C1=CC=CC=C1", true)),
        SmilesStyle::Recorded,
    );
    let (_, aromatic) = counted(
        &Input::of(&drawing("c1ccccc1", true)),
        SmilesStyle::Recorded,
    );
    assert!(
        aromatic > kekule,
        "aromatic benzene must ask the aromaticity model's questions on top of those of Kekulé benzene: aromatic asked {aromatic}, Kekulé asked {kekule}"
    );
}

#[test]
fn the_stereo_stage_asks_questions_in_the_recorded_style() {
    let text = "C[C@H](N)O |&1:1|";
    let (_, stereo) = counted(&Input::of(&drawing(text, true)), SmilesStyle::Recorded);
    let (_, skeleton) = counted(&Input::of(&drawing(text, false)), SmilesStyle::Recorded);
    assert!(
        stereo > skeleton,
        "the stored center and its group must add questions to the same skeleton without stereo: with stereo asked {stereo}, without asked {skeleton}"
    );
}

/// Writing CCO asks 6 questions in the recorded style and 7 in the canonical style. The writer
/// asked 4 and 6 before its read-back parses asked questions. Each read-back parse of CCO asks
/// one, since a text without aromatic bonds reaches one reader checkpoint: the recorded style
/// reads back twice (the render and the stereo read-back), so 4 + 2 = 6, and the canonical style
/// reads back once (the render), so 6 + 1 = 7.
#[test]
fn the_cco_write_asks_the_writer_and_read_back_questions() {
    let cco = Input::of(&drawing("CCO", true));
    for (style, expected) in [(SmilesStyle::Recorded, 6), (SmilesStyle::Canonical, 7)] {
        let (outcome, asked) = counted(&cco, style);
        assert!(
            matches!(outcome, Ok(Ok(_))),
            "CCO in {style:?} must be written: {outcome:?}"
        );
        assert_eq!(
            asked, expected,
            "CCO in {style:?} must ask the writer's questions and one per read-back parse"
        );
    }
}
