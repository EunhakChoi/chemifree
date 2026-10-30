//! A document's style settings: how its drawing is to be drawn (bond length, line width, label
//! font and size, which carbons are labelled) and in which unit lengths are shown.
//!
//! Settings belong to the document. They are saved with it, changed only by an edit
//! (`update_settings`), and undone and redone like any other change. Application-wide
//! preferences (the user interface, the settings new documents start from, computation) live
//! outside documents: a document copies its starting settings once, when it is created, and
//! later preference changes never reach it.
//!
//! Lengths are stored in points (1/72 inch) whatever unit they are shown in. Nothing here
//! computes chemistry or renders anything.

use std::fmt;
use std::ops::RangeInclusive;

/// Bond lengths, in pt, a document may draw new bonds at.
pub const BOND_LENGTH_RANGE: RangeInclusive<f64> = 5.0..=200.0;
/// Stroke widths, in pt.
pub const LINE_WIDTH_RANGE: RangeInclusive<f64> = 0.1..=10.0;
/// Atom label font sizes, in pt.
pub const LABEL_SIZE_RANGE: RangeInclusive<f64> = 4.0..=72.0;

/// The generic font family atom labels are drawn in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LabelFont {
    #[default]
    SansSerif,
    Serif,
    Monospace,
}

/// Which carbons are drawn with a label.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CarbonLabels {
    /// A bonded carbon that records nothing is a bare vertex; every other carbon is labelled.
    #[default]
    Default,
    /// Every carbon is labelled.
    All,
}

/// The unit lengths are shown in. It changes nothing stored: lengths are always kept in pt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayUnit {
    #[default]
    Pt,
    Mm,
    Cm,
    In,
}

/// A document's settings. [`Settings::default`] is the built-in default every document starts
/// from unless preferences name other values.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The length new bonds are drawn at, in pt.
    pub bond_length_pt: f64,
    /// The stroke width of bonds and graphics, in pt.
    pub line_width_pt: f64,
    pub label_font: LabelFont,
    /// The font size of atom labels, in pt.
    pub label_size_pt: f64,
    pub carbon_labels: CarbonLabels,
    pub display_unit: DisplayUnit,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bond_length_pt: 30.0,
            line_width_pt: 1.2,
            label_font: LabelFont::SansSerif,
            label_size_pt: 12.0,
            carbon_labels: CarbonLabels::Default,
            display_unit: DisplayUnit::Pt,
        }
    }
}

impl Settings {
    /// Checks every value; edits, restored files, replayed history, and preferences all pass
    /// through here.
    pub fn validate(&self) -> Result<(), SettingsError> {
        let within =
            |range: &RangeInclusive<f64>, value: f64| value.is_finite() && range.contains(&value);
        if !within(&BOND_LENGTH_RANGE, self.bond_length_pt) {
            return Err(SettingsError::BondLengthOutOfRange);
        }
        if !within(&LINE_WIDTH_RANGE, self.line_width_pt) {
            return Err(SettingsError::LineWidthOutOfRange);
        }
        if !within(&LABEL_SIZE_RANGE, self.label_size_pt) {
            return Err(SettingsError::LabelSizeOutOfRange);
        }
        Ok(())
    }

    /// These settings with the fields `edit` names replaced, checked as a whole.
    pub fn edited(&self, edit: &SettingsEdit) -> Result<Self, SettingsError> {
        let edited = Self {
            bond_length_pt: edit.bond_length_pt.unwrap_or(self.bond_length_pt),
            line_width_pt: edit.line_width_pt.unwrap_or(self.line_width_pt),
            label_font: edit.label_font.unwrap_or(self.label_font),
            label_size_pt: edit.label_size_pt.unwrap_or(self.label_size_pt),
            carbon_labels: edit.carbon_labels.unwrap_or(self.carbon_labels),
            display_unit: edit.display_unit.unwrap_or(self.display_unit),
        };
        edited.validate()?;
        Ok(edited)
    }

    /// The edit that names every field, so applying it to any settings gives these.
    pub fn to_edit(&self) -> SettingsEdit {
        SettingsEdit {
            bond_length_pt: Some(self.bond_length_pt),
            line_width_pt: Some(self.line_width_pt),
            label_font: Some(self.label_font),
            label_size_pt: Some(self.label_size_pt),
            carbon_labels: Some(self.carbon_labels),
            display_unit: Some(self.display_unit),
        }
    }
}

/// Settings fields to change; fields left `None` keep their values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsEdit {
    pub bond_length_pt: Option<f64>,
    pub line_width_pt: Option<f64>,
    pub label_font: Option<LabelFont>,
    pub label_size_pt: Option<f64>,
    pub carbon_labels: Option<CarbonLabels>,
    pub display_unit: Option<DisplayUnit>,
}

/// A refused settings value, with its stable code. The codes are documented in
/// docs/CONTRACT.md.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsError {
    /// A bond length outside [`BOND_LENGTH_RANGE`] or not finite.
    BondLengthOutOfRange,
    /// A line width outside [`LINE_WIDTH_RANGE`] or not finite.
    LineWidthOutOfRange,
    /// A label size outside [`LABEL_SIZE_RANGE`] or not finite.
    LabelSizeOutOfRange,
}

impl SettingsError {
    /// Every code, in documentation order.
    pub const ALL: [Self; 3] = [
        Self::BondLengthOutOfRange,
        Self::LineWidthOutOfRange,
        Self::LabelSizeOutOfRange,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::BondLengthOutOfRange => "bond_length_out_of_range",
            Self::LineWidthOutOfRange => "line_width_out_of_range",
            Self::LabelSizeOutOfRange => "label_size_out_of_range",
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (what, range) = match self {
            Self::BondLengthOutOfRange => ("the bond length", BOND_LENGTH_RANGE),
            Self::LineWidthOutOfRange => ("the line width", LINE_WIDTH_RANGE),
            Self::LabelSizeOutOfRange => ("the label size", LABEL_SIZE_RANGE),
        };
        write!(
            formatter,
            "{what} must be a finite number from {} to {} pt",
            range.start(),
            range.end()
        )
    }
}

impl std::error::Error for SettingsError {}
