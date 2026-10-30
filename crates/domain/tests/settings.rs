//! Document settings: every document carries its own, changed only by `update_settings`, which
//! is an edit like any other (one revision, one history entry, undone and redone as one step);
//! refused values name their code and change nothing; restored documents obey the same rules.

use chemifree_domain::document::Atom;
use chemifree_domain::document::{
    Change, Command, Document, DocumentId, EditError, NewAtomPositions, Outcome, Point,
    TransactionError,
};
use chemifree_domain::element::Element;
use chemifree_domain::settings::{
    CarbonLabels, DisplayUnit, LabelFont, Settings, SettingsEdit, SettingsError,
};

fn empty() -> Document {
    Document::new(DocumentId::parse("doc-settings").unwrap(), "Settings").unwrap()
}

fn update(edit: SettingsEdit) -> Command {
    Command::UpdateSettings(edit)
}

fn refusal(document: &mut Document, commands: &[Command]) -> TransactionError {
    let before = document.clone();
    let error = document
        .apply(commands)
        .expect_err("the transaction must be refused");
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

#[test]
fn a_new_document_has_the_built_in_defaults() {
    let settings = empty().settings().clone();
    assert_eq!(
        settings,
        Settings {
            bond_length_pt: 30.0,
            line_width_pt: 1.2,
            label_font: LabelFont::SansSerif,
            label_size_pt: 12.0,
            carbon_labels: CarbonLabels::Default,
            display_unit: DisplayUnit::Pt,
        }
    );
    assert_eq!(settings, Settings::default());
}

#[test]
fn a_document_can_start_from_given_settings_and_refuses_invalid_ones() {
    let start = Settings {
        bond_length_pt: 40.0,
        label_font: LabelFont::Serif,
        ..Settings::default()
    };
    let document = Document::new_with_settings(
        DocumentId::parse("doc-start").unwrap(),
        "Start",
        start.clone(),
    )
    .unwrap();
    assert_eq!(document.settings(), &start);
    assert_eq!(document.revision(), 0);
    assert_eq!(
        document.undo_depth(),
        0,
        "starting settings are not an edit"
    );

    let error = Document::new_with_settings(
        DocumentId::parse("doc-start").unwrap(),
        "Start",
        Settings {
            line_width_pt: 0.0,
            ..Settings::default()
        },
    )
    .expect_err("a line width of 0 is refused");
    assert_eq!(
        error,
        EditError::Settings(SettingsError::LineWidthOutOfRange)
    );
}

#[test]
fn update_settings_changes_only_the_named_fields_as_one_undoable_edit() {
    let mut document = empty();
    let outcome = document
        .apply(&[update(SettingsEdit {
            bond_length_pt: Some(45.5),
            carbon_labels: Some(CarbonLabels::All),
            display_unit: Some(DisplayUnit::Mm),
            ..SettingsEdit::default()
        })])
        .unwrap();
    assert_eq!(outcome, Outcome::Changed);
    let edited = Settings {
        bond_length_pt: 45.5,
        carbon_labels: CarbonLabels::All,
        display_unit: DisplayUnit::Mm,
        ..Settings::default()
    };
    assert_eq!(document.settings(), &edited);
    assert_eq!((document.revision(), document.undo_depth()), (1, 1));

    document.undo().unwrap();
    assert_eq!(document.settings(), &Settings::default());
    assert_eq!(document.revision(), 2);
    document.redo().unwrap();
    assert_eq!(document.settings(), &edited);
    assert_eq!(document.revision(), 3);
}

#[test]
fn settings_and_drawing_edits_in_one_transaction_are_one_step() {
    let mut document = empty();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            },
            update(SettingsEdit {
                label_size_pt: Some(18.0),
                ..SettingsEdit::default()
            }),
        ])
        .unwrap();
    document.undo().unwrap();
    assert_eq!(document.atoms().len(), 0);
    assert_eq!(document.settings().label_size_pt, 12.0);
}

#[test]
fn an_update_that_changes_nothing_is_a_no_op() {
    let mut document = empty();
    for edit in [
        SettingsEdit::default(),
        SettingsEdit {
            bond_length_pt: Some(30.0),
            label_font: Some(LabelFont::SansSerif),
            ..SettingsEdit::default()
        },
    ] {
        assert_eq!(
            document.apply(&[update(edit.clone())]).unwrap(),
            Outcome::Unchanged,
            "{edit:?}"
        );
    }
    assert_eq!((document.revision(), document.undo_depth()), (0, 0));
}

#[test]
fn out_of_range_values_are_refused_with_their_code_and_change_nothing() {
    let cases = [
        (
            SettingsEdit {
                bond_length_pt: Some(4.99),
                ..SettingsEdit::default()
            },
            SettingsError::BondLengthOutOfRange,
        ),
        (
            SettingsEdit {
                bond_length_pt: Some(200.01),
                ..SettingsEdit::default()
            },
            SettingsError::BondLengthOutOfRange,
        ),
        (
            SettingsEdit {
                bond_length_pt: Some(f64::NAN),
                ..SettingsEdit::default()
            },
            SettingsError::BondLengthOutOfRange,
        ),
        (
            SettingsEdit {
                line_width_pt: Some(0.09),
                ..SettingsEdit::default()
            },
            SettingsError::LineWidthOutOfRange,
        ),
        (
            SettingsEdit {
                line_width_pt: Some(f64::INFINITY),
                ..SettingsEdit::default()
            },
            SettingsError::LineWidthOutOfRange,
        ),
        (
            SettingsEdit {
                label_size_pt: Some(3.5),
                ..SettingsEdit::default()
            },
            SettingsError::LabelSizeOutOfRange,
        ),
        (
            SettingsEdit {
                label_size_pt: Some(72.5),
                ..SettingsEdit::default()
            },
            SettingsError::LabelSizeOutOfRange,
        ),
    ];
    for (edit, expected) in cases {
        let mut document = empty();
        let error = refusal(
            &mut document,
            &[
                update(SettingsEdit {
                    bond_length_pt: Some(40.0),
                    ..SettingsEdit::default()
                }),
                update(edit.clone()),
            ],
        );
        assert_eq!(
            error,
            TransactionError {
                index: Some(1),
                error: EditError::Settings(expected),
            },
            "{edit:?}"
        );
        assert_eq!(error.error.reason(), Some(expected.code()), "{edit:?}");
    }
}

#[test]
fn the_range_bounds_are_accepted() {
    let mut document = empty();
    for (bond, line, label) in [(5.0, 0.1, 4.0), (200.0, 10.0, 72.0)] {
        document
            .apply(&[update(SettingsEdit {
                bond_length_pt: Some(bond),
                line_width_pt: Some(line),
                label_size_pt: Some(label),
                ..SettingsEdit::default()
            })])
            .unwrap();
        assert_eq!(
            (
                document.settings().bond_length_pt,
                document.settings().line_width_pt,
                document.settings().label_size_pt
            ),
            (bond, line, label)
        );
    }
}

#[test]
fn reason_codes_are_stable_and_distinct() {
    let codes: Vec<&str> = SettingsError::ALL
        .iter()
        .map(|error| error.code())
        .collect();
    assert_eq!(
        codes,
        [
            "bond_length_out_of_range",
            "line_width_out_of_range",
            "label_size_out_of_range"
        ]
    );
}

#[test]
fn an_edit_overlays_settings_and_a_full_edit_reproduces_them() {
    let custom = Settings {
        bond_length_pt: 25.0,
        line_width_pt: 2.0,
        label_font: LabelFont::Monospace,
        label_size_pt: 9.5,
        carbon_labels: CarbonLabels::All,
        display_unit: DisplayUnit::In,
    };
    assert_eq!(
        Settings::default().edited(&custom.to_edit()),
        Ok(custom.clone())
    );
    assert_eq!(
        custom.edited(&SettingsEdit {
            label_font: Some(LabelFont::Serif),
            ..SettingsEdit::default()
        }),
        Ok(Settings {
            label_font: LabelFont::Serif,
            ..custom
        })
    );
    assert_eq!(
        Settings::default().edited(&SettingsEdit {
            label_size_pt: Some(100.0),
            ..SettingsEdit::default()
        }),
        Err(SettingsError::LabelSizeOutOfRange)
    );
}

#[test]
fn copies_and_restored_documents_keep_their_settings_and_history() {
    let mut document = empty();
    document
        .apply(&[update(SettingsEdit {
            line_width_pt: Some(2.5),
            label_font: Some(LabelFont::Serif),
            ..SettingsEdit::default()
        })])
        .unwrap();
    let restored = Document::restore(document.to_parts()).unwrap();
    assert_eq!(restored, document);
    let mut restored = restored;
    restored.undo().unwrap();
    assert_eq!(restored.settings(), &Settings::default());

    let copy = document
        .copy_as(DocumentId::parse("doc-copy").unwrap(), "Copy")
        .unwrap();
    assert_eq!(copy.settings(), document.settings());
}

#[test]
fn restoring_refuses_invalid_settings_in_the_document_or_its_history() {
    let mut parts = empty().to_parts();
    parts.settings.bond_length_pt = 300.0;
    let error = Document::restore(parts).expect_err("stored settings out of range");
    assert!(
        error.to_string().contains("bond length"),
        "the reason names the setting: {error}"
    );

    let mut document = empty();
    document
        .apply(&[update(SettingsEdit {
            bond_length_pt: Some(40.0),
            ..SettingsEdit::default()
        })])
        .unwrap();
    let mut parts = document.to_parts();
    parts.undo[0].settings = Some(Change {
        before: Settings {
            label_size_pt: 1.0,
            ..Settings::default()
        },
        after: document.settings().clone(),
    });
    assert!(
        Document::restore(parts).is_err(),
        "undo history leading to invalid settings is refused"
    );

    let mut parts = document.to_parts();
    parts.undo[0].settings = Some(Change {
        before: Settings::default(),
        after: Settings::default(),
    });
    assert!(
        Document::restore(parts).is_err(),
        "a settings change that changes nothing is not a well-formed entry"
    );
}
