//! Unit tests of the overlap rule of P48 S4 and of the `generated_layout_overlap` losses it feeds
//! (CONTRACT, P48 S4). Each test names the behavior it protects.

use std::collections::BTreeSet;

use super::*;

/// A list of overlapping atom pairs, as `overlap_pairs` returns it.
type Pairs = Vec<(usize, usize)>;

/// The reason of a `generated_layout_overlap` loss, written out from the V14 text. `listed` is the
/// text inside the parentheses after the count, including any `, and N more`.
fn expected_reason(count: usize, listed: &str) -> String {
    format!(
        "Non-bonded atom pairs closer than 0.3 bond lengths in the generated layout ({}): {count} ({listed}). Overlaps are reported, not resolved; the generated positions are kept.",
        chemifree_domain::layout::MODEL_ID
    )
}

#[test]
fn overlap_pairs_reports_only_pairs_strictly_inside_the_limit() {
    let limit = IMPORT_OVERLAP_FACTOR * 10.0;
    let cases: [(&str, f64, Pairs); 2] = [
        // Exactly at the limit is not an overlap: the comparison is strict.
        ("at the limit", limit, Vec::new()),
        ("just inside the limit", limit * 0.999, vec![(0, 1)]),
    ];
    for (label, distance, expected) in cases {
        let shape = [(0.0, 0.0), (distance, 0.0)];
        let found = overlap_pairs(&shape, &BTreeSet::new(), 10.0);
        assert_eq!(
            found, expected,
            "{label}: distance {distance} against the limit {limit}"
        );
    }
}

#[test]
fn overlap_pairs_never_reports_a_bonded_pair_even_when_coincident() {
    let shape = [(0.0, 0.0), (0.0, 0.0)];
    // Control: without the bond the same coincident pair is reported, so the empty result below
    // comes from the bond and nothing else.
    assert_eq!(overlap_pairs(&shape, &BTreeSet::new(), 10.0), vec![(0, 1)]);
    let bonded = BTreeSet::from([(0, 1)]);
    let found = overlap_pairs(&shape, &bonded, 10.0);
    assert!(
        found.is_empty(),
        "a bonded pair is not an overlap, got {found:?}"
    );
}

#[test]
fn overlap_loss_names_the_component_its_count_and_its_first_twenty_pairs() {
    // Twenty-two coincident, unbonded atoms: each of their 22 * 21 / 2 = 231 pairs overlaps.
    let shape = vec![(0.0, 0.0); 22];
    let overlaps = overlap_pairs(&shape, &BTreeSet::new(), 30.0);
    assert_eq!(
        overlaps.len(),
        231,
        "every pair of 22 coincident atoms overlaps"
    );
    let first: Vec<(usize, usize)> = (1..=20).map(|j| (0, j)).collect();
    let listed_pairs: Vec<(usize, usize)> = overlaps.iter().take(20).copied().collect();
    assert_eq!(
        listed_pairs, first,
        "the first twenty pairs ascend from atom 1"
    );
    let all_atoms: Vec<String> = (1..=22).map(|atom| atom.to_string()).collect();
    let listed: Vec<String> = (2..=21).map(|j| format!("1-{j}")).collect();
    let plan = ComponentPlan {
        atoms: (0..22).collect(),
        shape,
        fallback: None,
        overlaps,
    };
    let losses = overlap_losses(std::slice::from_ref(&plan), "records[2].");
    let [loss] = losses.as_slice() else {
        panic!("one laid-out component with overlaps gives one loss, got {losses:?}");
    };
    assert_eq!(loss.feature, "generated_layout_overlap");
    assert_eq!(
        loss.source_path,
        format!("records[2].atoms[{}]", all_atoms.join(", "))
    );
    assert_eq!(
        loss.reason,
        expected_reason(231, &format!("{}, and 211 more", listed.join(", ")))
    );
    assert_eq!(loss.disposition, LossDisposition::Normalized);
    assert_eq!(loss.semantic_impact, SemanticImpact::Geometry);
    assert_eq!(loss.consent, LossConsent::NotRequired);
}

#[test]
fn overlap_loss_at_exactly_twenty_pairs_has_no_more_suffix() {
    // Two clusters of five coincident atoms, 100 pt apart: 10 + 10 = 20 overlapping pairs.
    let shape: Vec<(f64, f64)> = [(0.0, 0.0); 5]
        .into_iter()
        .chain([(100.0, 0.0); 5])
        .collect();
    let overlaps = overlap_pairs(&shape, &BTreeSet::new(), 30.0);
    let plan = ComponentPlan {
        atoms: (0..10).collect(),
        shape,
        fallback: None,
        overlaps,
    };
    assert_eq!(
        plan.overlaps.len(),
        20,
        "two clusters of five give 10 + 10 pairs"
    );
    let losses = overlap_losses(std::slice::from_ref(&plan), "");
    let [loss] = losses.as_slice() else {
        panic!("one laid-out component with overlaps gives one loss, got {losses:?}");
    };
    assert_eq!(loss.source_path, "atoms[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]");
    assert_eq!(
        loss.reason,
        expected_reason(
            20,
            "1-2, 1-3, 1-4, 1-5, 2-3, 2-4, 2-5, 3-4, 3-5, 4-5, \
             6-7, 6-8, 6-9, 6-10, 7-8, 7-9, 7-10, 8-9, 8-10, 9-10"
        )
    );
}

#[test]
fn a_fallback_component_without_overlaps_gives_no_loss() {
    let kinds = [
        (Fallback::Grid("x"), grid_shape(2)),
        (Fallback::SourcePoint("x"), vec![(0.0, 0.0); 2]),
    ];
    for (fallback, shape) in kinds {
        let plan = ComponentPlan {
            atoms: vec![0, 1],
            shape,
            fallback: Some(fallback),
            overlaps: Vec::new(),
        };
        let losses = overlap_losses(std::slice::from_ref(&plan), "");
        assert!(
            losses.is_empty(),
            "a {fallback:?} component without overlaps reports no overlap, got {losses:?}"
        );
    }
}

#[test]
fn each_overlapping_plan_gives_one_loss_in_plan_order() {
    let plans = [
        ComponentPlan {
            atoms: vec![0, 1, 2],
            shape: vec![(0.0, 0.0); 3],
            fallback: None,
            overlaps: vec![(0, 1), (0, 2), (1, 2)],
        },
        ComponentPlan {
            atoms: vec![3, 4],
            shape: grid_shape(2),
            fallback: Some(Fallback::Grid("x")),
            overlaps: Vec::new(),
        },
        ComponentPlan {
            atoms: vec![5, 6],
            shape: vec![(0.0, 0.0); 2],
            fallback: None,
            overlaps: vec![(5, 6)],
        },
    ];
    let losses = overlap_losses(&plans, "");
    let [first, second] = losses.as_slice() else {
        panic!("two overlapping plans give two losses, got {losses:?}");
    };
    assert_eq!(first.source_path, "atoms[1, 2, 3]");
    assert_eq!(first.reason, expected_reason(3, "1-2, 1-3, 2-3"));
    assert_eq!(second.source_path, "atoms[6, 7]");
    assert_eq!(second.reason, expected_reason(1, "6-7"));
}
