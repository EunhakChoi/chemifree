//! The measurements of the three generic label fonts, copied from `METRICS` in
//! `apps/web/src/editor/label.ts`: per family, an upper bound of each character's advance (as a
//! share of the font size) and how far a run's box may reach past its advance at either end. The
//! numbers are a reservation, not a promise about any viewer's glyphs: a viewer's generic family
//! may be wider, so text can reach past the box it was given. This file is the only place the
//! tables live; a change here changes every exported drawing and the parity fixture.

use crate::LabelFontDto;

/// Names the measurements below. Change it with any value here: an exported drawing carries it.
pub const ADVANCE_TABLE_ID: &str = "chemifree-label-advance-1";

/// The advance of a character that no row lists, as a share of the font size (`UNKNOWN_ADVANCE`).
const UNKNOWN_ADVANCE: f64 = 1.1;

/// Half the height a run may take around its middle, as a share of its font size (`HALF_HEIGHT`).
pub(super) const HALF_HEIGHT: f64 = 0.7;

/// Rows of characters, each with the advance upper bound its characters share. A character that two
/// rows list takes the advance of the later row, as the `Map` built in label.ts does.
type Table = &'static [(&'static str, f64)];

const SANS_SERIF: Table = &[
    ("i", 0.27),
    ("Ijl!,", 0.29),
    ("[]", 0.37),
    ("r", 0.4),
    ("ft", 0.42),
    ("*", 0.44),
    ("s", 0.56),
    ("yz17", 0.57),
    ("cv", 0.58),
    ("a?", 0.59),
    ("Le+−", 0.6),
    ("hnou", 0.61),
    ("6", 0.62),
    ("FJTZbdgkpqx9", 0.63),
    ("8", 0.64),
    ("234", 0.65),
    ("5", 0.66),
    ("EKPSXY0", 0.69),
    ("ABV", 0.71),
    ("CDHRU", 0.74),
    ("N", 0.77),
    ("GOQ", 0.8),
    ("w", 0.85),
    ("Mm", 0.91),
    ("W", 0.99),
    ("…", 1.02),
];

const SERIF: Table = &[
    ("j", 0.32),
    ("il,", 0.33),
    ("f", 0.38),
    ("[]", 0.4),
    ("I!tJ", 0.41),
    ("r", 0.49),
    ("*", 0.51),
    ("s", 0.53),
    ("z", 0.54),
    ("?", 0.55),
    ("ycvx", 0.58),
    ("ae", 0.61),
    ("ok", 0.62),
    ("0123456789", 0.65),
    ("hnubdgpq", 0.66),
    ("L", 0.68),
    ("TP", 0.69),
    ("S", 0.7),
    ("FZ", 0.71),
    ("XYAV", 0.74),
    ("EB", 0.75),
    ("KR", 0.77),
    ("C", 0.79),
    ("DG", 0.82),
    ("OQ", 0.84),
    ("+−U", 0.86),
    ("w", 0.88),
    ("H", 0.89),
    ("N", 0.9),
    ("m", 0.97),
    ("…", 1.02),
    ("MW", 1.05),
];

const MONOSPACE: Table = &[(
    "0123456789iIjl!,[]rft*syzcva?Le+−hnouFJTZbdgkpqxEKPSXYABVCDHRUNGOQwMmW…",
    0.62,
)];

/// The generic CSS family a label font is drawn in (`family` in the canvas). The drawing names the
/// generic keyword only, so each viewer chooses its own face for it.
pub(super) fn family(font: LabelFontDto) -> &'static str {
    match font {
        LabelFontDto::SansSerif => "sans-serif",
        LabelFontDto::Serif => "serif",
        LabelFontDto::Monospace => "monospace",
    }
}

fn table(font: LabelFontDto) -> Table {
    match font {
        LabelFontDto::SansSerif => SANS_SERIF,
        LabelFontDto::Serif => SERIF,
        LabelFontDto::Monospace => MONOSPACE,
    }
}

/// How far a run's box may reach past its advance at either end, as a share of the font size.
pub(super) fn overhang(font: LabelFontDto) -> f64 {
    match font {
        LabelFontDto::SansSerif | LabelFontDto::Monospace => 0.04,
        LabelFontDto::Serif => 0.11,
    }
}

/// The advance upper bound of `character`, as a share of the font size: the last row that lists
/// it, or [`UNKNOWN_ADVANCE`] when none does.
pub(super) fn advance(font: LabelFontDto, character: char) -> f64 {
    lookup(table(font), character)
}

/// The advance `table` gives `character`: the last row that lists it, else [`UNKNOWN_ADVANCE`].
fn lookup(table: Table, character: char) -> f64 {
    table
        .iter()
        .rev()
        .find(|(characters, _)| characters.contains(character))
        .map_or(UNKNOWN_ADVANCE, |(_, advance)| *advance)
}

/// An upper bound of the advance of `text` drawn at `size` in `font` (`textWidth`), summed from
/// the first character to the last.
pub(super) fn text_width(text: &str, size: f64, font: LabelFontDto) -> f64 {
    text.chars().fold(0.0, |total, character| {
        total + advance(font, character) * size
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_row_overrides_an_earlier_one() {
        let rows: Table = &[("ab", 0.5), ("b", 0.7)];
        assert_eq!(lookup(rows, 'a'), 0.5);
        assert_eq!(
            lookup(rows, 'b'),
            0.7,
            "the later row lists b, so its 0.7 wins"
        );
    }

    #[test]
    fn each_family_keeps_its_own_advance() {
        // "*" is 0.44 in sans-serif and 0.51 in serif.
        assert_eq!(advance(LabelFontDto::SansSerif, '*'), 0.44);
        assert_eq!(advance(LabelFontDto::Serif, '*'), 0.51);
    }

    #[test]
    fn an_unlisted_character_takes_the_unknown_advance() {
        assert_eq!(advance(LabelFontDto::Monospace, 'Ω'), UNKNOWN_ADVANCE);
        assert_eq!(advance(LabelFontDto::SansSerif, 'ß'), UNKNOWN_ADVANCE);
    }

    #[test]
    fn text_width_sums_each_character_at_the_size() {
        // In sans-serif, 'H' is 0.74 (row CDHRU) and '3' is 0.65 (row 234).
        let width = text_width("H3", 12.0, LabelFontDto::SansSerif);
        assert!((width - (0.74 + 0.65) * 12.0).abs() < 1e-9, "got {width}");
    }

    #[test]
    fn families_have_the_documented_names() {
        assert_eq!(family(LabelFontDto::SansSerif), "sans-serif");
        assert_eq!(family(LabelFontDto::Serif), "serif");
        assert_eq!(family(LabelFontDto::Monospace), "monospace");
    }
}
