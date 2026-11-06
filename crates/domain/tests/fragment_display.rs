//! Explicit graph contraction accepts trees, never interprets display text, and reveals edits.

use chemifree_domain::clipboard::{self, FragmentError};
use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    EditError, FragmentDisplay, NewAtomPositions, ObjectId, Point, PositionKey, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use chemifree_domain::scene::Selection;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use chemifree_domain::stereo::StereoMark;

fn atom(id: u32) -> AtomId {
    AtomId::from_serial(id).unwrap()
}
fn instance() -> ObjectId {
    ObjectId::from_serial(1).unwrap()
}
fn drawing(symbols: &[&str], edges: &[(u32, u32)]) -> Document {
    let mut doc = Document::new(DocumentId::parse("fragment").unwrap(), "Explicit graph").unwrap();
    let mut commands: Vec<Command> = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
            positions: NewAtomPositions::Single(
                Point::new(index as f64 * 30.0, index as f64 * 7.0).unwrap(),
            ),
            bond_to: None,
        })
        .collect();
    commands.extend(edges.iter().map(|(begin, end)| Command::AddBond {
        begin: AtomRef::Created((*begin - 1) as usize),
        end: AtomRef::Created((*end - 1) as usize),
        order: BondOrder::Single,
    }));
    doc.apply(&commands).unwrap();
    doc
}
fn contract(members: &[u32], text: &str) -> Command {
    Command::ContractFragment {
        instance: instance(),
        anchor: atom(2),
        members: members.iter().copied().map(atom).collect(),
        text: text.into(),
    }
}
fn branched() -> Document {
    drawing(&["O", "C", "C", "C"], &[(1, 2), (2, 3), (2, 4)])
}
fn text(text: &str) -> Command {
    Command::SetFragmentDisplayText {
        instance: instance(),
        anchor: atom(2),
        text: text.into(),
    }
}
fn refuse(doc: &mut Document, command: Command) {
    let before = doc.clone();
    assert!(
        doc.apply(&[
            Command::Rename {
                title: "rollback".into()
            },
            command
        ])
        .is_err()
    );
    assert_eq!(
        *doc, before,
        "all content, counters, selection and history roll back"
    );
}

#[test]
fn named_and_unicode_displays_keep_the_exact_graph_coordinates_ids_and_identity() {
    for (symbols, edges, members, label) in [
        (
            vec!["O", "C", "C", "C"],
            vec![(1, 2), (2, 3), (3, 4)],
            vec![2, 3, 4],
            "nPr",
        ),
        (
            vec!["O", "C", "C", "C"],
            vec![(1, 2), (2, 3), (2, 4)],
            vec![2, 3, 4],
            "iPr",
        ),
        (
            vec!["O", "C", "C", "C", "C"],
            vec![(1, 2), (2, 3), (2, 4), (2, 5)],
            vec![2, 3, 4, 5],
            "tBu",
        ),
        (vec!["C", "O", "C"], vec![(1, 2), (2, 3)], vec![2, 3], "OMe"),
        (
            vec!["C", "N", "O", "C"],
            vec![(1, 2), (2, 3), (2, 4)],
            vec![2, 3, 4],
            "한 e\u{301} 🧪",
        ),
        // A one-atom tree is the smallest admissible explicit member set.
        (vec!["C", "O"], vec![(1, 2)], vec![2], "Ph"),
    ] {
        let mut doc = drawing(&symbols, &edges);
        let before = doc.to_parts();
        let identity = identity::canonicalize(&MoleculeGraph::from_document(&doc))
            .unwrap()
            .code;
        doc.apply(&[contract(&members, label)]).unwrap();
        let after = doc.to_parts();
        assert_eq!(after.atoms, before.atoms);
        assert_eq!(after.bonds, before.bonds);
        assert_eq!(after.positions, before.positions);
        assert_eq!(after.stereo, before.stereo);
        assert_eq!(after.stereo_groups, before.stereo_groups);
        assert_eq!(after.settings, before.settings);
        assert_eq!(
            (after.next_atom, after.next_bond, after.next_object),
            (before.next_atom, before.next_bond, before.next_object)
        );
        assert_eq!(
            identity::canonicalize(&MoleculeGraph::from_document(&doc))
                .unwrap()
                .code,
            identity
        );
        assert_eq!(doc.fragment_displays().next().unwrap().1.text, label);
        assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
        doc.undo().unwrap();
        assert_eq!(doc.fragment_displays().count(), 0);
        doc.redo().unwrap();
        assert_eq!(doc.fragment_displays().count(), 1);
    }
}

#[test]
fn text_edit_is_explicit_and_never_guesses_a_graph_or_a_default() {
    let mut doc = branched();
    doc.apply(&[contract(&[4, 2, 3], "iPr")]).unwrap();
    assert_eq!(
        doc.fragment_displays().next().unwrap().1.members,
        vec![atom(2), atom(3), atom(4)]
    );
    let graph = doc.to_parts();
    doc.apply(&[text("unrelated chemistry? 한 🧪")]).unwrap();
    assert_eq!(doc.to_parts().atoms, graph.atoms);
    assert_eq!(doc.to_parts().bonds, graph.bonds);
    doc.undo().unwrap();
    assert_eq!(doc.fragment_displays().next().unwrap().1.text, "iPr");
    doc.redo().unwrap();
    for invalid in [
        "".to_owned(),
        "   ".into(),
        "a\nb".into(),
        "a\tb".into(),
        "한".repeat(43),
        "a\u{2028}b".into(),
    ] {
        refuse(&mut doc, text(&invalid));
    }
    doc.apply(&[text(&"\\\"".repeat(64))]).unwrap();
    doc.apply(&[Command::ExpandFragment {
        instance: instance(),
        anchor: atom(2),
    }])
    .unwrap();
    assert_eq!(doc.fragment_displays().count(), 0);
    refuse(&mut doc, text("x"));
    doc.undo().unwrap();
    assert_eq!(doc.fragment_displays().count(), 1);
}

#[test]
fn disconnected_cycles_bad_boundary_duplicate_and_missing_anchor_fail_atomically() {
    for (mut doc, members) in [
        (branched(), vec![]),
        (branched(), vec![3, 4]),
        (branched(), vec![2, 2, 3, 4]),
        (branched(), vec![2, 3]),
        (branched(), vec![1, 2, 3, 4]),
        (branched(), vec![2, 3, 99]),
        (
            drawing(&["O", "C", "C", "C"], &[(1, 2), (2, 3), (3, 4), (4, 2)]),
            vec![2, 3, 4],
        ),
        // Edge count alone cannot prove connectivity: a disconnected cycle plus the anchor.
        (
            drawing(
                &["O", "C", "C", "C", "C"],
                &[(1, 2), (3, 4), (4, 5), (5, 3)],
            ),
            vec![2, 3, 4, 5],
        ),
    ] {
        refuse(&mut doc, contract(&members, "label"));
    }
}

#[test]
fn nondefault_atom_states_and_marked_or_nonsingle_edges_refuse_contraction() {
    let mut states = Vec::new();
    let base = Atom::from(Element::CARBON);
    let mut query = base.clone();
    query.kind = AtomKind::Query(QueryAtom::Any);
    states.push(query);
    let mut charge = base.clone();
    charge.charge = 1;
    states.push(charge);
    let mut isotope = base.clone();
    isotope.isotope = Some(13);
    states.push(isotope);
    let mut hydrogen = base.clone();
    hydrogen.hydrogens = Hydrogens::Specified(1);
    states.push(hydrogen);
    let mut radical = base.clone();
    radical.radical = Radical::Doublet;
    states.push(radical);
    let mut attachment = base;
    attachment.attachment_points = AttachmentPoints::new(&[1]).unwrap();
    states.push(attachment);
    for state in states {
        let mut doc = branched();
        let mut parts = doc.to_parts();
        parts
            .atoms
            .iter_mut()
            .find(|(id, _)| *id == atom(3))
            .unwrap()
            .1 = state;
        parts.undo.clear();
        doc = Document::restore(parts).unwrap();
        refuse(&mut doc, contract(&[2, 3, 4], "iPr"));
    }
    for bond in [1, 2] {
        let mut doc = branched();
        doc.apply(&[Command::SetBondMark {
            bond: BondRef::Id(BondId::from_serial(bond).unwrap()),
            mark: Some(StereoMark::Wavy),
        }])
        .unwrap();
        refuse(&mut doc, contract(&[2, 3, 4], "iPr"));
        let mut doc = branched();
        doc.apply(&[Command::UpdateBond {
            bond: BondId::from_serial(bond).unwrap(),
            order: BondOrder::Double,
        }])
        .unwrap();
        refuse(&mut doc, contract(&[2, 3, 4], "iPr"));
    }
}

#[test]
fn whole_member_moves_keep_display_partial_and_boundary_edits_reveal_with_undo() {
    let mut doc = branched();
    doc.apply(&[contract(&[2, 3, 4], "iPr")]).unwrap();
    doc.apply(&[Command::MoveAtoms {
        atoms: vec![atom(2), atom(3), atom(4)],
        instance: Some(instance()),
        delta: Vector { dx: 3.0, dy: 4.0 },
    }])
    .unwrap();
    assert_eq!(doc.fragment_displays().count(), 1);
    for command in [
        Command::MoveAtoms {
            atoms: vec![atom(3)],
            instance: Some(instance()),
            delta: Vector { dx: 1.0, dy: 0.0 },
        },
        Command::UpdateAtom {
            atom: atom(3),
            edit: AtomEdit {
                charge: Some(1),
                ..AtomEdit::default()
            },
        },
        Command::UpdateAtom {
            atom: atom(1),
            edit: AtomEdit {
                charge: Some(1),
                ..AtomEdit::default()
            },
        },
        Command::UpdateBond {
            bond: BondId::from_serial(1).unwrap(),
            order: BondOrder::Double,
        },
        Command::DeleteAtoms {
            atoms: vec![atom(4)],
        },
        Command::AddInstance {
            source: Some(instance()),
            offset: Vector { dx: 100.0, dy: 0.0 },
        },
    ] {
        let before = doc.clone();
        doc.apply(&[command]).unwrap();
        assert_eq!(doc.fragment_displays().count(), 0);
        doc.undo().unwrap();
        assert_eq!(
            doc.to_parts().fragment_displays,
            before.to_parts().fragment_displays
        );
        assert_eq!(doc.to_parts().atoms, before.to_parts().atoms);
        assert_eq!(doc.to_parts().positions, before.to_parts().positions);
    }
}

#[test]
fn overlap_with_ethyl_or_fragment_and_multiple_placement_refuse() {
    let mut doc = drawing(&["O", "C", "C"], &[(1, 2), (2, 3)]);
    doc.apply(&[Command::CollapseEthyl {
        instance: instance(),
        anchor: atom(2),
        terminal: atom(3),
    }])
    .unwrap();
    refuse(&mut doc, contract(&[2, 3], "OMe"));
    doc.undo().unwrap();
    doc.apply(&[contract(&[2, 3], "arbitrary")]).unwrap();
    refuse(
        &mut doc,
        Command::CollapseEthyl {
            instance: instance(),
            anchor: atom(2),
            terminal: atom(3),
        },
    );
    refuse(
        &mut doc,
        Command::ContractFragment {
            instance: instance(),
            anchor: atom(3),
            members: vec![atom(3)],
            text: "nested".into(),
        },
    );
    doc.apply(&[Command::AddInstance {
        source: Some(instance()),
        offset: Vector { dx: 100.0, dy: 0.0 },
    }])
    .unwrap();
    refuse(&mut doc, contract(&[2, 3], "x"));
}

#[test]
fn native_fragment_copy_refuses_any_member_but_allows_outside_selection() {
    let mut doc = branched();
    doc.apply(&[contract(&[2, 3, 4], "iPr")]).unwrap();
    for target in [2, 3, 4] {
        let selection = Selection {
            atoms: [atom(target)].into(),
            ..Selection::default()
        };
        assert_eq!(
            clipboard::copy_fragment(&doc, &selection, instance()),
            Err(FragmentError::AbbreviationDisplay)
        );
    }
    assert!(
        clipboard::copy_fragment(
            &doc,
            &Selection {
                atoms: [atom(1)].into(),
                ..Selection::default()
            },
            instance()
        )
        .is_ok()
    );
}

#[test]
fn malformed_current_and_trimmed_history_metadata_cannot_be_restored() {
    let mut doc = branched();
    doc.apply(&[contract(&[2, 3, 4], "iPr")]).unwrap();
    let mut parts = doc.to_parts();
    parts.fragment_displays[0].1.members.push(atom(2));
    assert!(Document::restore(parts).is_err());
    let mut parts = doc.to_parts();
    let mut invalid = parts.undo.last().unwrap().clone();
    invalid.fragment_displays[0].1.after = Some(FragmentDisplay {
        members: vec![atom(2)],
        text: "\n".into(),
    });
    // An invalid oldest record would otherwise be discarded by count retention.
    parts.undo.splice(0..0, std::iter::repeat_n(invalid, 101));
    assert!(Document::restore(parts).is_err());
    let mut parts = doc.to_parts();
    parts.fragment_displays[0].0 = PositionKey {
        instance: ObjectId::from_serial(99).unwrap(),
        atom: atom(2),
    };
    assert!(Document::restore(parts).is_err());
    assert_eq!(
        EditError::FragmentDisplayInvalid
            .scene_error()
            .unwrap()
            .code(),
        "fragment_display_invalid"
    );
}

#[test]
fn template_attachment_and_replacement_refuse_hidden_or_visible_display_members() {
    let mut doc = branched();
    let copied = clipboard::copy_fragment(
        &doc,
        &Selection {
            atoms: [atom(1)].into(),
            ..Selection::default()
        },
        instance(),
    )
    .unwrap();
    doc.apply(&[contract(&[2, 3, 4], "iPr")]).unwrap();
    let before = doc.clone();
    for target in [atom(2), atom(3), atom(4)] {
        let attachment = chemifree_domain::template::attach(
            &copied.fragment,
            &doc,
            instance(),
            atom(1),
            target,
            Vector { dx: 1.0, dy: 0.0 },
        );
        assert_eq!(
            attachment.unwrap_err().reason(),
            "clipboard_abbreviation_display_unsupported"
        );
        let replacement = chemifree_domain::template::replace(
            &copied.fragment,
            &doc,
            instance(),
            atom(1),
            target,
        );
        assert_eq!(
            replacement.unwrap_err().reason(),
            "clipboard_abbreviation_display_unsupported"
        );
    }
    assert_eq!(doc, before);
}

#[test]
fn unrelated_stereo_is_preserved_and_boundary_neighbor_marks_reveal_the_display() {
    let mut doc = drawing(
        &["O", "C", "C", "C", "C", "C"],
        &[(1, 2), (2, 3), (2, 4), (5, 6)],
    );
    doc.apply(&[Command::SetBondMark {
        bond: BondRef::Id(BondId::from_serial(4).unwrap()),
        mark: Some(StereoMark::Wavy),
    }])
    .unwrap();
    let graph = doc.to_parts();
    doc.apply(&[contract(&[2, 3, 4], "iPr")]).unwrap();
    assert_eq!(doc.to_parts().bonds, graph.bonds);
    assert_eq!(doc.to_parts().stereo, graph.stereo);
    doc.apply(&[Command::AddBond {
        begin: AtomRef::Id(atom(1)),
        end: AtomRef::Id(atom(5)),
        order: BondOrder::Single,
    }])
    .unwrap();
    assert_eq!(doc.fragment_displays().count(), 1);
    doc.apply(&[Command::SetBondMark {
        bond: BondRef::Id(BondId::from_serial(5).unwrap()),
        mark: Some(StereoMark::Wavy),
    }])
    .unwrap();
    assert_eq!(doc.fragment_displays().count(), 0);
    doc.undo().unwrap();
    assert_eq!(doc.fragment_displays().count(), 1);
}
