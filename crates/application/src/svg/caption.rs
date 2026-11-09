//! The layout and markup of a caption in the bounded SVG export (P38b2): which characters are
//! drawn, the style of each run, the box a caption reserves in the view box, and the elements that
//! draw it. The run rules mirror `SceneCaption` in `apps/web/src/editor/scene-caption.tsx`, and the
//! reservation models the measurement in `apps/web/src/editor/structure-canvas.tsx`.
//!
//! A caption is drawn only when every character is in the set `drawable` accepts. Those characters
//! need no shaping, combining or bidirectional handling, so a generic font sets each one from left
//! to right. A caption with any other character is not drawn in part: it has no layout and is
//! reported as a loss (`losses.rs`).
//!
//! The reserved box is a model, not the ink. A viewer's generic face may be wider or taller than
//! the advance table predicts, so its text can reach past the box, as label text can.
//! The advances ignore a run's weight and style, and a character the table does not list takes its
//! fixed fallback advance, so the box can also be wider than the text.

use serde::Serialize;

use super::font;
use super::geometry::{self, LabelBox};
use super::writer;
use crate::{
    CaptionAlignmentDto, CaptionRunDto, CaptionScriptDto, CaptionStyleDto, CaptionWeightDto,
    LabelFontDto, PositionView, SettingsDto,
};

/// How far a reserved box reaches above a run's baseline, in em of the run's glyph size. A model
/// value; with `CAPTION_DESCENT` it spans 1.4 em, twice the label's `HALF_HEIGHT`.
const CAPTION_ASCENT: f64 = 1.0;
/// How far a reserved box reaches below a run's baseline, in em of the run's glyph size. A model
/// value, as `CAPTION_ASCENT` is.
const CAPTION_DESCENT: f64 = 0.4;
/// The padding on each side of a reserved box, in pt. `structure-canvas.tsx:335` pads a measured
/// caption by 2.
const CAPTION_PADDING: f64 = 2.0;
/// The colour of a run with no stored colour. The theme foreground is black in an exported drawing.
const DEFAULT_COLOR: &str = "#000000";
/// The opacity of a caption's shadow group, as the shadow layer of `SceneCaption` in
/// `scene-caption.tsx` sets it.
const SHADOW_OPACITY: f64 = 0.35;

/// One run of a drawn caption, with the attributes its `tspan` carries (`renderText` in
/// `scene-caption.tsx`, lines 27-35).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaptionRunLayout {
    pub text: String,
    /// The generic CSS family the run is drawn in.
    pub family: &'static str,
    /// The glyph size in pt: the run's size, or two thirds of it for a sub- or superscript.
    pub size: f64,
    /// The shift of the run from the caption's baseline in pt. A positive shift moves down.
    pub shift: f64,
    /// `normal` or `bold`.
    pub weight: &'static str,
    /// `normal` or `italic`.
    pub style: &'static str,
    /// The stored colour in `#RRGGBB`, or `DEFAULT_COLOR` when none is stored.
    pub color: String,
    /// The stroke width in pt of an outlined run, or None for a filled run.
    pub outline: Option<f64>,
    /// Whether the run has a shadow, which the export draws as a copy at +1 pt.
    pub shadow: bool,
    /// Whether the run is underlined. The export does not draw underlines.
    pub underline: bool,
}

/// A drawn caption: its anchor, its runs, and the box it reserves.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaptionLayout {
    pub id: String,
    /// The anchor point of the caption in pt, on its baseline.
    pub x: f64,
    pub y: f64,
    /// The SVG `text-anchor` of the caption: `start`, `middle` or `end`.
    pub anchor: &'static str,
    pub runs: Vec<CaptionRunLayout>,
    /// Whether any run is shadowed.
    pub shadow: bool,
    /// The reserved box in pt. It is a reservation, not ink (see `reservation`).
    pub bounds: LabelBox,
}

/// Whether `character` is in the set a caption draws: printable ASCII; Latin-1 except the soft
/// hyphen (U+00AD), which is a format character; the basic Greek letters; and the en and em dashes,
/// the prime and double prime, the ellipsis, the two arrows, the equilibrium arrow and the minus
/// sign.
pub(super) fn drawable(character: char) -> bool {
    matches!(
        character,
        ' '..='~'
            | '\u{00A0}'..='\u{00AC}'
            | '\u{00AE}'..='\u{00FF}'
            | '\u{0391}'..='\u{03A1}'
            | '\u{03A3}'..='\u{03A9}'
            | '\u{03B1}'..='\u{03C9}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{2026}'
            | '\u{2032}'
            | '\u{2033}'
            | '\u{2190}'
            | '\u{2192}'
            | '\u{21CC}'
            | '\u{2212}'
    )
}

/// Whether every character of every run is drawn (`drawable`). A caption that is not is not drawn
/// at all, so no part of its text reaches the export.
pub(super) fn caption_drawable(runs: &[CaptionRunDto]) -> bool {
    runs.iter().all(|run| run.text.chars().all(drawable))
}

/// The layout of the caption `id` at `anchor`, aligned as `alignment` aligns it, or None when the
/// caption is not drawn: a character is outside the drawn set (`caption_drawable`), or the caption
/// has no run. The domain rejects a caption without runs, so a view built from a document never
/// has one.
///
/// Each run follows `renderText` in `scene-caption.tsx`: its size falls back to the label size, a
/// sub- or superscript is two thirds of it, a subscript sits a third of its size below the baseline
/// and a superscript 5/12 of it above, and an outline is a sixteenth of its glyph size. A stored
/// colour is drawn as it is.
pub(super) fn layout_caption(
    settings: &SettingsDto,
    id: &str,
    anchor: PositionView,
    alignment: CaptionAlignmentDto,
    runs: &[CaptionRunDto],
) -> Option<CaptionLayout> {
    if !caption_drawable(runs) {
        return None;
    }
    let laid_out: Vec<(LabelFontDto, CaptionRunLayout)> = runs
        .iter()
        .map(|run| (run_font(settings, run), run_layout(settings, run)))
        .collect();
    let shadow = laid_out.iter().any(|(_, run)| run.shadow);
    let bounds = reservation(anchor, alignment, shadow, &laid_out)?;
    Some(CaptionLayout {
        id: id.to_owned(),
        x: anchor.x,
        y: anchor.y,
        anchor: match alignment {
            CaptionAlignmentDto::Left => "start",
            CaptionAlignmentDto::Center => "middle",
            CaptionAlignmentDto::Right => "end",
        },
        runs: laid_out.into_iter().map(|(_, run)| run).collect(),
        shadow,
        bounds,
    })
}

/// The font a run is set in: its own, or else the label font (`run.font ?? settings.label_font`).
fn run_font(settings: &SettingsDto, run: &CaptionRunDto) -> LabelFontDto {
    run.font.unwrap_or(settings.label_font)
}

/// The attributes of one run, as `renderText` gives them.
fn run_layout(settings: &SettingsDto, run: &CaptionRunDto) -> CaptionRunLayout {
    let run_size = run.size_pt.unwrap_or(settings.label_size_pt);
    let (size, shift) = match run.script {
        CaptionScriptDto::Baseline => (run_size, 0.0),
        CaptionScriptDto::Subscript => (run_size * 2.0 / 3.0, run_size / 3.0),
        CaptionScriptDto::Superscript => (run_size * 2.0 / 3.0, -run_size * 5.0 / 12.0),
    };
    CaptionRunLayout {
        text: run.text.clone(),
        family: font::family(run_font(settings, run)),
        size,
        shift,
        weight: match run.weight {
            CaptionWeightDto::Normal => "normal",
            CaptionWeightDto::Bold => "bold",
        },
        style: match run.style {
            CaptionStyleDto::Normal => "normal",
            CaptionStyleDto::Italic => "italic",
        },
        color: run.color.as_deref().unwrap_or(DEFAULT_COLOR).to_owned(),
        outline: run.outline.then_some(size / 16.0),
        shadow: run.shadow,
        underline: run.underline,
    }
}

/// The box a caption reserves (`bounds`), or None when it has no run. It is a reservation, not ink:
/// the web measures the ink (`structure-canvas.tsx:279-335`), and the export models it instead. The
/// runs' advance in the label table (`font::text_width`) is aligned at `anchor` as `alignment` says.
/// The box reaches the first run's overhang before the start and the last run's after the end, as a
/// label's box reaches past its advance (`chunk_box` in `label.rs`). Vertically it spans each run's
/// ascent and descent around its shift. It then grows by the widest outline's half width, by the
/// shadow's 1 pt on the right and bottom, and by the padding on every side.
fn reservation(
    anchor: PositionView,
    alignment: CaptionAlignmentDto,
    shadow: bool,
    laid_out: &[(LabelFontDto, CaptionRunLayout)],
) -> Option<LabelBox> {
    let (first_face, first) = laid_out.first()?;
    let (last_face, last) = laid_out.last()?;
    let mut advance = 0.0;
    let mut top = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let mut grow: f64 = 0.0;
    for (face, run) in laid_out {
        advance += font::text_width(&run.text, run.size, *face);
        top = top.min(anchor.y + run.shift - CAPTION_ASCENT * run.size);
        bottom = bottom.max(anchor.y + run.shift + CAPTION_DESCENT * run.size);
        grow = grow.max(run.outline.map_or(0.0, |outline| outline / 2.0));
    }
    let offset = match alignment {
        CaptionAlignmentDto::Left => 0.0,
        CaptionAlignmentDto::Center => advance / 2.0,
        CaptionAlignmentDto::Right => advance,
    };
    let left = anchor.x - offset - font::overhang(*first_face) * first.size;
    let right = anchor.x - offset + advance + font::overhang(*last_face) * last.size;
    let shadow_offset = if shadow { 1.0 } else { 0.0 };
    let left = left - grow - CAPTION_PADDING;
    let top = top - grow - CAPTION_PADDING;
    let right = right + grow + shadow_offset + CAPTION_PADDING;
    let bottom = bottom + grow + shadow_offset + CAPTION_PADDING;
    Some(LabelBox {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// The markup of a drawn caption, as lines for `writer::document` to join, one element per line.
/// The shadow comes before the caption, so the caption paints over it.
///
/// The caption is one `text` element on one line, as `writer.rs` writes an atom label. Its runs are
/// `tspan`s with a relative `dy` and no `x` or `y`, so the caption stays one text chunk and
/// `text-anchor` aligns all of it. Underlines are not drawn, so no markup is written for them.
pub(super) fn caption_markup(layout: &CaptionLayout) -> Vec<String> {
    let mut lines = vec!["<g>".to_owned()];
    if layout.shadow {
        lines.push(format!(
            r#"<g opacity="{}">"#,
            writer::number(SHADOW_OPACITY)
        ));
        lines.push(caption_text(
            layout,
            layout.x + 1.0,
            layout.y + 1.0,
            /* shadow_copy= */ true,
        ));
        lines.push("</g>".to_owned());
    }
    lines.push(caption_text(
        layout, layout.x, layout.y, /* shadow_copy= */ false,
    ));
    lines.push("</g>".to_owned());
    lines
}

/// One caption `text` element at `(x, y)`. In the shadow copy (`shadow_copy`), a run without a
/// shadow is `visibility="hidden"`; it keeps its place in the text layout, so the shadowed runs
/// stay where they are. Each run's `dy` is its rounded shift less the rounded shift of the run
/// before it, as `text_element` in `writer.rs` computes it, and a zero `dy` is left out.
fn caption_text(layout: &CaptionLayout, x: f64, y: f64, shadow_copy: bool) -> String {
    let mut tspans: Vec<String> = Vec::new();
    let mut previous = 0.0;
    for run in &layout.runs {
        let shift = geometry::round2(run.shift);
        let step = shift - previous;
        previous = shift;
        let dy = if geometry::round2(step) == 0.0 {
            String::new()
        } else {
            format!(r#" dy="{}""#, writer::number(step))
        };
        let paint = match run.outline {
            Some(width) => format!(
                r#" fill="none" stroke="{}" stroke-width="{}" stroke-linejoin="round""#,
                writer::escape(&run.color),
                writer::number(width)
            ),
            None => format!(r#" fill="{}""#, writer::escape(&run.color)),
        };
        let hidden = if shadow_copy && !run.shadow {
            r#" visibility="hidden""#
        } else {
            ""
        };
        tspans.push(format!(
            r#"<tspan{dy} font-family="{}" font-size="{}" font-weight="{}" font-style="{}"{paint}{hidden}>{}</tspan>"#,
            run.family,
            writer::number(run.size),
            run.weight,
            run.style,
            writer::escape(&run.text)
        ));
    }
    format!(
        r#"<text x="{}" y="{}" text-anchor="{}" xml:space="preserve">{}</text>"#,
        writer::number(x),
        writer::number(y),
        layout.anchor,
        tspans.concat()
    )
}
