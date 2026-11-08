//! The label of an atom (`atomLabel` and `layout` in `apps/web/src/editor/label.ts`), in normal
//! typography: upright, weight 500 for the symbol and `H`, weight 400 for the small runs. Radical
//! dots, the `?` of an unknown radical and attachment points are not drawn and are not in the box.
//! Bold, italic, underline, outline and shadow are drawn as normal typography; P38a2 reports them
//! as a loss.

use serde::Serialize;

use super::font;
use super::geometry::{self, LabelBox};
use crate::{
    AtomKindView, AtomView, CarbonLabelsDto, HydrogensDto, LabelFontDto, QueryDto, RadicalDto,
};

/// The built-in label size: a layout is made at it and then scaled (`LABEL_SIZE`).
const LABEL_SIZE: f64 = 12.0;
/// The size of hydrogen counts, isotopes and charges (`SMALL_SIZE`).
const SMALL_SIZE: f64 = 8.0;
/// How far a label's box reaches past its content, at the built-in size (`PADDING`).
const PADDING: f64 = 1.0;
/// How many elements of a list query the canvas names (`SHOWN_QUERY_ELEMENTS`).
pub(super) const SHOWN_QUERY_ELEMENTS: usize = 3;
/// The weight of the symbol and of `H` (the 500 of `layout`).
const SYMBOL_WEIGHT: u16 = 500;
/// The weight of the hydrogen count, the isotope and the charge (the default 400 of `chemicalRun`).
const SMALL_WEIGHT: u16 = 400;
/// The shift of a hydrogen count, which sits below the baseline. A positive shift moves down.
const COUNT_SHIFT: f64 = 4.0;
/// The shift of an isotope and of a charge, which sit above the baseline.
const SCRIPT_SHIFT: f64 = -5.0;
/// The minus sign the canvas writes for a negative charge (`chargeText`).
const MINUS_SIGN: char = '\u{2212}';

/// How the labels of a drawing are set: the font, the label size in pt, and which carbons are
/// labelled (the parts of `LabelStyle` that the export reads from the document's settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LabelStyle {
    pub(super) font: LabelFontDto,
    pub(super) size: f64,
    pub(super) carbons: CarbonLabelsDto,
}

/// One run of text in a label: its text, its font size and its shift from the symbol's baseline,
/// both in pt (a positive shift is downward), and its weight.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LabelRun {
    pub text: String,
    pub size: f64,
    pub shift: f64,
    pub weight: u16,
}

/// The label of one atom at one size (`AtomLabel` in label.ts, without its radical and attachment
/// marks). The text is centred at `x` from the atom and at the atom's height. `chunk_box` is the
/// box the text reserves and `bounds` the label box with its padding; both are relative to the atom.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomLabel {
    pub x: f64,
    pub runs: Vec<LabelRun>,
    #[serde(rename = "chunkBox")]
    pub chunk_box: LabelBox,
    #[serde(rename = "box")]
    pub bounds: LabelBox,
}

/// Whether an atom is labelled (`showsLabel`). With the default carbon labels a bonded carbon that
/// records nothing is a bare vertex; every other atom is labelled, and with `all` every carbon too.
pub(super) fn shows_label(atom: &AtomView, degree: usize, carbons: CarbonLabelsDto) -> bool {
    carbons == CarbonLabelsDto::All
        || atom.kind == AtomKindView::Query
        || atom.element != "C"
        || degree == 0
        || has_states(atom)
}

/// Whether an atom records a state the canvas labels it for (`statesDescription` is not empty):
/// stated or unknown hydrogens, a radical, attachment points, a charge or an isotope.
fn has_states(atom: &AtomView) -> bool {
    !matches!(atom.hydrogens, HydrogensDto::Inferred {})
        || atom.radical != RadicalDto::None
        || !atom.attachment_points.is_empty()
        || atom.charge != 0
        || atom.isotope.is_some()
}

/// The label a query atom shows on the canvas (`queryLabel`): a named query its letter, and a list
/// its first three elements, then `,…` when there are more, in brackets and after `!` when negated.
/// The untruncated text of `AtomView.element` is not used here.
pub(super) fn canvas_query_label(query: &QueryDto) -> String {
    match query {
        QueryDto::Any {} => "*".to_owned(),
        QueryDto::AnyHeavy {} => "A".to_owned(),
        QueryDto::Heteroatom {} => "Q".to_owned(),
        QueryDto::Halogen {} => "X".to_owned(),
        QueryDto::List { elements, negated } => {
            let shown: Vec<&str> = elements
                .iter()
                .take(SHOWN_QUERY_ELEMENTS)
                .map(String::as_str)
                .collect();
            let more = if elements.len() > SHOWN_QUERY_ELEMENTS {
                ",…"
            } else {
                ""
            };
            let negation = if *negated { "!" } else { "" };
            format!("{negation}[{}{more}]", shown.join(","))
        }
    }
}

/// The charge written after a symbol (`chargeText`): nothing for zero, a sign alone for one, and the
/// magnitude before the sign otherwise (`2+`, `2−`).
pub(super) fn charge_text(charge: i8) -> String {
    if charge == 0 {
        return String::new();
    }
    let sign = if charge > 0 { '+' } else { MINUS_SIGN };
    let magnitude = charge.unsigned_abs();
    if magnitude == 1 {
        sign.to_string()
    } else {
        format!("{magnitude}{sign}")
    }
}

/// The label of `atom` in `style`, or None when it is a bare carbon vertex (`atomLabel` without its
/// cache). `hydrogens_left` puts its stated hydrogens before the symbol, because its bonds lean
/// right. The layout is made at the built-in size and then scaled to the label size.
pub(super) fn atom_label(
    atom: &AtomView,
    degree: usize,
    hydrogens_left: bool,
    style: LabelStyle,
) -> Option<AtomLabel> {
    if !shows_label(atom, degree, style.carbons) {
        return None;
    }
    Some(scaled(
        layout(atom, hydrogens_left, style.font),
        style.size / LABEL_SIZE,
    ))
}

/// The label of `atom` at the built-in size (`layout`): the symbol, centred on the atom, with the
/// hydrogens, isotope and charge beside it.
fn layout(atom: &AtomView, hydrogens_left: bool, face: LabelFontDto) -> AtomLabel {
    let symbol = match (atom.kind, &atom.query) {
        (AtomKindView::Query, Some(query)) => canvas_query_label(query),
        _ => atom.element.clone(),
    };
    let symbol_run = run(&symbol, LABEL_SIZE, 0.0, SYMBOL_WEIGHT);
    let hydrogens = hydrogen_runs(atom.hydrogens);
    let (mut before, mut after) = if hydrogens_left {
        (hydrogens, Vec::new())
    } else {
        (Vec::new(), hydrogens)
    };
    if let Some(isotope) = atom.isotope {
        before.push(run(
            &isotope.to_string(),
            SMALL_SIZE,
            SCRIPT_SHIFT,
            SMALL_WEIGHT,
        ));
    }
    let charge = charge_text(atom.charge);
    if !charge.is_empty() {
        after.push(run(&charge, SMALL_SIZE, SCRIPT_SHIFT, SMALL_WEIGHT));
    }
    // What precedes and what follows the symbol are drawn at their natural widths, so the text is
    // centred where their reserved widths centre the symbol on the atom.
    let centre = (runs_advance(&after, face) - runs_advance(&before, face)) / 2.0;
    let mut runs = before;
    runs.push(symbol_run);
    runs.extend(after);
    let reserved = chunk_box(&runs, face, centre);
    let content = geometry::union(&[reserved]);
    AtomLabel {
        x: centre,
        runs,
        chunk_box: reserved,
        bounds: LabelBox {
            x: content.x - PADDING,
            y: content.y - PADDING,
            width: content.width + 2.0 * PADDING,
            height: content.height + 2.0 * PADDING,
        },
    }
}

/// The hydrogen runs of an atom: `H`, then its count as a small subscript, `?` when unknown, and
/// no count when it is one. Inferred hydrogens have no runs.
fn hydrogen_runs(hydrogens: HydrogensDto) -> Vec<LabelRun> {
    let count = match hydrogens {
        HydrogensDto::Inferred {} => return Vec::new(),
        HydrogensDto::Unknown {} => "?".to_owned(),
        HydrogensDto::Specified { count: 1 } => String::new(),
        HydrogensDto::Specified { count } => count.to_string(),
    };
    let mut runs = vec![run("H", LABEL_SIZE, 0.0, SYMBOL_WEIGHT)];
    if !count.is_empty() {
        runs.push(run(&count, SMALL_SIZE, COUNT_SHIFT, SMALL_WEIGHT));
    }
    runs
}

/// A run of upright text.
fn run(text: &str, size: f64, shift: f64, weight: u16) -> LabelRun {
    LabelRun {
        text: text.to_owned(),
        size,
        shift,
        weight,
    }
}

/// The advance of `runs` in `face`, each at its own size, summed from the first run to the last.
fn runs_advance(runs: &[LabelRun], face: LabelFontDto) -> f64 {
    runs.iter().fold(0.0, |total, item| {
        total + font::text_width(&item.text, item.size, face)
    })
}

/// The box a centred chunk of `runs` reserves (`chunk`, normal path): its advance centred on `x`,
/// a height from each run's shift and half height, and the font's overhang at both ends.
fn chunk_box(runs: &[LabelRun], face: LabelFontDto, x: f64) -> LabelBox {
    let advance = runs_advance(runs, face);
    let overhang = runs.iter().fold(0.0, |most, item| {
        f64::max(most, font::overhang(face) * item.size)
    });
    let top = runs.iter().fold(f64::INFINITY, |least, item| {
        f64::min(least, item.shift - font::HALF_HEIGHT * item.size)
    });
    let bottom = runs.iter().fold(f64::NEG_INFINITY, |most, item| {
        f64::max(most, item.shift + font::HALF_HEIGHT * item.size)
    });
    LabelBox {
        x: x - advance / 2.0 - overhang,
        y: top,
        width: advance + overhang + overhang,
        height: bottom - top,
    }
}

/// `label` with every length in it multiplied by `factor` (`scaled`).
fn scaled(label: AtomLabel, factor: f64) -> AtomLabel {
    AtomLabel {
        x: label.x * factor,
        runs: label
            .runs
            .into_iter()
            .map(|item| LabelRun {
                text: item.text,
                size: item.size * factor,
                shift: item.shift * factor,
                weight: item.weight,
            })
            .collect(),
        chunk_box: scale_box(label.chunk_box, factor),
        bounds: scale_box(label.bounds, factor),
    }
}

fn scale_box(bounds: LabelBox, factor: f64) -> LabelBox {
    LabelBox {
        x: bounds.x * factor,
        y: bounds.y * factor,
        width: bounds.width * factor,
        height: bounds.height * factor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PositionView;

    const STYLE: LabelStyle = LabelStyle {
        font: LabelFontDto::SansSerif,
        size: 12.0,
        carbons: CarbonLabelsDto::Default,
    };

    fn atom(element: &str) -> AtomView {
        AtomView {
            id: "a1".to_owned(),
            element: element.to_owned(),
            charge: 0,
            isotope: None,
            position: PositionView { x: 0.0, y: 0.0 },
            kind: AtomKindView::Element,
            query: None,
            hydrogens: HydrogensDto::Inferred {},
            radical: RadicalDto::None,
            attachment_points: Vec::new(),
        }
    }

    fn list(elements: &[&str], negated: bool) -> QueryDto {
        QueryDto::List {
            elements: elements
                .iter()
                .map(|element| (*element).to_owned())
                .collect(),
            negated,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    /// Checks a box against `[x, y, width, height]`.
    fn assert_box(actual: LabelBox, expected: [f64; 4]) {
        assert_close(actual.x, expected[0]);
        assert_close(actual.y, expected[1]);
        assert_close(actual.width, expected[2]);
        assert_close(actual.height, expected[3]);
    }

    fn texts(label: &AtomLabel) -> Vec<&str> {
        label.runs.iter().map(|item| item.text.as_str()).collect()
    }

    #[test]
    fn charge_text_names_the_sign_and_the_magnitude_above_one() {
        assert_eq!(charge_text(0), "");
        assert_eq!(charge_text(1), "+");
        assert_eq!(charge_text(-1), "\u{2212}");
        assert_eq!(charge_text(2), "2+");
        assert_eq!(charge_text(-2), "2\u{2212}");
        assert_eq!(
            charge_text(i8::MIN),
            "128\u{2212}",
            "the magnitude of i8::MIN is 128, which does not fit in i8"
        );
    }

    #[test]
    fn canvas_query_label_names_three_elements_then_an_ellipsis() {
        assert_eq!(
            canvas_query_label(&list(&["C", "N", "O", "S"], false)),
            "[C,N,O,…]"
        );
        assert_eq!(
            canvas_query_label(&list(&["C", "N", "O"], false)),
            "[C,N,O]"
        );
        assert_eq!(
            canvas_query_label(&list(&["C", "N", "O", "S"], true)),
            "![C,N,O,…]"
        );
        assert_eq!(canvas_query_label(&QueryDto::Any {}), "*");
        assert_eq!(canvas_query_label(&QueryDto::AnyHeavy {}), "A");
        assert_eq!(canvas_query_label(&QueryDto::Heteroatom {}), "Q");
        assert_eq!(canvas_query_label(&QueryDto::Halogen {}), "X");
    }

    #[test]
    fn a_bonded_bare_carbon_is_unlabelled_unless_it_records_a_state() {
        let carbon = atom("C");
        assert!(
            !shows_label(&carbon, 2, CarbonLabelsDto::Default),
            "bonded bare carbon"
        );
        assert!(
            shows_label(&carbon, 0, CarbonLabelsDto::Default),
            "a carbon with no bond"
        );
        assert!(
            shows_label(&carbon, 2, CarbonLabelsDto::All),
            "every carbon with all"
        );
        assert!(
            shows_label(&atom("N"), 2, CarbonLabelsDto::Default),
            "a nitrogen"
        );

        let mut charged = atom("C");
        charged.charge = 1;
        assert!(
            shows_label(&charged, 2, CarbonLabelsDto::Default),
            "a charged carbon"
        );

        let mut inferred_count = atom("C");
        inferred_count.hydrogens = HydrogensDto::Specified { count: 0 };
        assert!(
            shows_label(&inferred_count, 2, CarbonLabelsDto::Default),
            "a stated count of zero is still a stated state"
        );

        let mut query = atom("C");
        query.kind = AtomKindView::Query;
        assert!(
            shows_label(&query, 2, CarbonLabelsDto::Default),
            "a query atom"
        );
    }

    #[test]
    fn a_neutral_nitrogen_reserves_its_advance_and_overhang_at_the_built_in_size() {
        // 'N' is 0.77 of the size in sans-serif (row "N"); the overhang is 0.04 of the size. The
        // text is 9.24 wide, so it starts at -4.62, and its box adds 0.48 on each side.
        let label = atom_label(&atom("N"), 1, false, STYLE).expect("a nitrogen is labelled");
        assert_close(label.x, 0.0);
        assert_eq!(texts(&label), ["N"]);
        assert_eq!(label.runs[0].size, 12.0);
        assert_eq!(label.runs[0].shift, 0.0);
        assert_eq!(label.runs[0].weight, 500);
        assert_box(label.chunk_box, [-5.10, -8.4, 10.2, 16.8]);
        assert_box(label.bounds, [-6.1, -9.4, 12.2, 18.8]);
    }

    #[test]
    fn scaling_multiplies_every_length_of_the_label() {
        let style = LabelStyle {
            size: 18.0,
            ..STYLE
        };
        let label = atom_label(&atom("N"), 1, false, style).expect("a nitrogen is labelled");
        assert_eq!(label.runs[0].size, 18.0);
        assert_box(label.chunk_box, [-7.65, -12.6, 15.3, 25.2]);
        assert_box(label.bounds, [-9.15, -14.1, 18.3, 28.2]);
    }

    #[test]
    fn hydrogens_on_the_left_come_before_the_symbol_and_the_symbol_stays_on_the_atom() {
        // 'H' is 0.74 and '3' is 0.65 of their sizes: the hydrogens are 14.08 wide at the sizes 12
        // and 8, so the chunk centres at -7.04 and the symbol's 9.24 starts at -4.62 after them.
        let mut hydrogens = atom("N");
        hydrogens.hydrogens = HydrogensDto::Specified { count: 3 };

        let left = atom_label(&hydrogens, 1, true, STYLE).expect("a nitrogen is labelled");
        assert_eq!(texts(&left), ["H", "3", "N"]);
        assert_close(left.x, -7.04);
        assert_box(left.chunk_box, [-19.18, -8.4, 24.28, 18.0]);

        let right = atom_label(&hydrogens, 1, false, STYLE).expect("a nitrogen is labelled");
        assert_eq!(texts(&right), ["N", "H", "3"]);
        assert_close(right.x, 7.04);
        assert_box(right.chunk_box, [-5.10, -8.4, 24.28, 18.0]);
    }

    #[test]
    fn an_isotope_and_a_charge_sit_beside_the_symbol() {
        // Isotope 13 (superscript, size 8) before, charge + after: the before run is 9.76 wide and
        // the after run 4.8 wide, so the chunk centres at -2.48 and its box is 19.0 high.
        let mut labelled = atom("C");
        labelled.isotope = Some(13);
        labelled.charge = 1;
        let label = atom_label(&labelled, 1, false, STYLE).expect("an isotope is a state");
        assert_eq!(texts(&label), ["13", "C", "+"]);
        assert_eq!(label.runs[0].shift, -5.0);
        assert_eq!(label.runs[2].shift, -5.0);
        assert_close(label.x, -2.48);
        assert_box(label.chunk_box, [-14.68, -10.6, 24.40, 19.0]);
    }

    #[test]
    fn unknown_hydrogens_show_a_question_mark_and_a_count_of_one_is_left_out() {
        let mut unknown = atom("N");
        unknown.hydrogens = HydrogensDto::Unknown {};
        let label = atom_label(&unknown, 1, false, STYLE).expect("unknown hydrogens are a state");
        assert_eq!(texts(&label), ["N", "H", "?"]);
        assert_eq!(label.runs[2].size, 8.0);
        assert_eq!(label.runs[2].shift, 4.0);
        assert_eq!(label.runs[2].weight, 400);

        let mut one = atom("N");
        one.hydrogens = HydrogensDto::Specified { count: 1 };
        let label = atom_label(&one, 1, false, STYLE).expect("a stated hydrogen is a state");
        assert_eq!(texts(&label), ["N", "H"]);
    }

    #[test]
    fn a_list_query_is_labelled_with_its_shortened_canvas_text_not_its_element() {
        let mut query = atom("C,N,O,S");
        query.kind = AtomKindView::Query;
        query.query = Some(list(&["C", "N", "O", "S"], false));
        let label = atom_label(&query, 1, false, STYLE).expect("a query atom is labelled");
        assert_eq!(texts(&label), ["[C,N,O,…]"]);
        assert_eq!(label.runs[0].weight, 500);
    }
}
