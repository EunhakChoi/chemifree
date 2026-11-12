//! Document settings as stored, and their conversion to and from the domain's settings.

use chemifree_domain::settings::{
    CarbonLabels, DisplayUnit, LabelFont, LabelStyle, LabelWeight, Settings,
};
use serde::{Deserialize, Serialize};

/// A document's settings, every field present. Lengths are in pt.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredSettings {
    bond_length_pt: f64,
    line_width_pt: f64,
    label_font: StoredFont,
    #[serde(default)]
    label_weight: StoredLabelWeight,
    #[serde(default)]
    label_style: StoredLabelStyle,
    #[serde(default)]
    label_underline: bool,
    #[serde(default)]
    label_outline: bool,
    #[serde(default)]
    label_shadow: bool,
    label_size_pt: f64,
    carbon_labels: StoredCarbonLabels,
    display_unit: StoredUnit,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredLabelWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredLabelStyle {
    #[default]
    Normal,
    Italic,
}

impl From<LabelWeight> for StoredLabelWeight {
    fn from(weight: LabelWeight) -> Self {
        match weight {
            LabelWeight::Normal => Self::Normal,
            LabelWeight::Bold => Self::Bold,
        }
    }
}

impl From<StoredLabelWeight> for LabelWeight {
    fn from(weight: StoredLabelWeight) -> Self {
        match weight {
            StoredLabelWeight::Normal => Self::Normal,
            StoredLabelWeight::Bold => Self::Bold,
        }
    }
}

impl From<LabelStyle> for StoredLabelStyle {
    fn from(style: LabelStyle) -> Self {
        match style {
            LabelStyle::Normal => Self::Normal,
            LabelStyle::Italic => Self::Italic,
        }
    }
}

impl From<StoredLabelStyle> for LabelStyle {
    fn from(style: StoredLabelStyle) -> Self {
        match style {
            StoredLabelStyle::Normal => Self::Normal,
            StoredLabelStyle::Italic => Self::Italic,
        }
    }
}

// The settings enums are shared with the preferences file, which stores the same values.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StoredFont {
    SansSerif,
    Serif,
    Monospace,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredCarbonLabels {
    Default,
    All,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StoredUnit {
    Pt,
    Mm,
    Cm,
    In,
}

impl From<LabelFont> for StoredFont {
    fn from(font: LabelFont) -> Self {
        match font {
            LabelFont::SansSerif => Self::SansSerif,
            LabelFont::Serif => Self::Serif,
            LabelFont::Monospace => Self::Monospace,
        }
    }
}

impl From<StoredFont> for LabelFont {
    fn from(font: StoredFont) -> Self {
        match font {
            StoredFont::SansSerif => Self::SansSerif,
            StoredFont::Serif => Self::Serif,
            StoredFont::Monospace => Self::Monospace,
        }
    }
}

impl From<CarbonLabels> for StoredCarbonLabels {
    fn from(labels: CarbonLabels) -> Self {
        match labels {
            CarbonLabels::Default => Self::Default,
            CarbonLabels::All => Self::All,
        }
    }
}

impl From<StoredCarbonLabels> for CarbonLabels {
    fn from(labels: StoredCarbonLabels) -> Self {
        match labels {
            StoredCarbonLabels::Default => Self::Default,
            StoredCarbonLabels::All => Self::All,
        }
    }
}

impl From<DisplayUnit> for StoredUnit {
    fn from(unit: DisplayUnit) -> Self {
        match unit {
            DisplayUnit::Pt => Self::Pt,
            DisplayUnit::Mm => Self::Mm,
            DisplayUnit::Cm => Self::Cm,
            DisplayUnit::In => Self::In,
        }
    }
}

impl From<StoredUnit> for DisplayUnit {
    fn from(unit: StoredUnit) -> Self {
        match unit {
            StoredUnit::Pt => Self::Pt,
            StoredUnit::Mm => Self::Mm,
            StoredUnit::Cm => Self::Cm,
            StoredUnit::In => Self::In,
        }
    }
}

pub(super) fn stored_settings(settings: &Settings) -> StoredSettings {
    StoredSettings {
        bond_length_pt: settings.bond_length_pt,
        line_width_pt: settings.line_width_pt,
        label_font: settings.label_font.into(),
        label_weight: settings.label_weight.into(),
        label_style: settings.label_style.into(),
        label_underline: settings.label_underline,
        label_outline: settings.label_outline,
        label_shadow: settings.label_shadow,
        label_size_pt: settings.label_size_pt,
        carbon_labels: settings.carbon_labels.into(),
        display_unit: settings.display_unit.into(),
    }
}

/// Stored settings as the domain's; `Document::restore` checks their ranges.
pub(super) fn to_settings(stored: &StoredSettings) -> Settings {
    Settings {
        bond_length_pt: stored.bond_length_pt,
        line_width_pt: stored.line_width_pt,
        label_font: stored.label_font.into(),
        label_weight: stored.label_weight.into(),
        label_style: stored.label_style.into(),
        label_underline: stored.label_underline,
        label_outline: stored.label_outline,
        label_shadow: stored.label_shadow,
        label_size_pt: stored.label_size_pt,
        carbon_labels: stored.carbon_labels.into(),
        display_unit: stored.display_unit.into(),
    }
}
