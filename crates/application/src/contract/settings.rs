//! Document settings shapes, settings requests and their domain conversions.

use chemifree_domain::settings::{
    CarbonLabels, DisplayUnit, LabelFont, LabelStyle, LabelWeight, Settings, SettingsEdit,
};
use serde::{Deserialize, Serialize};

use super::request::SelectionDto;

/// The generic font family of atom labels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabelFontDto {
    SansSerif,
    Serif,
    Monospace,
}

/// The chemical label weight; independent captions retain their own run weight.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelWeightDto {
    #[default]
    Normal,
    Bold,
}

/// The chemical label style; independent captions retain their own run style.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStyleDto {
    #[default]
    Normal,
    Italic,
}

/// Which carbons are labelled: `default` leaves a bonded carbon that records nothing bare.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CarbonLabelsDto {
    Default,
    All,
}

/// The unit lengths are shown in; stored lengths are always pt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayUnitDto {
    Pt,
    Mm,
    Cm,
    In,
}

/// A document's settings, every field present; lengths in pt. Added within v1.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDto {
    pub bond_length_pt: f64,
    pub line_width_pt: f64,
    pub label_font: LabelFontDto,
    /// Pre-weight clipboard/template payloads inherit normal; explicit null remains invalid.
    #[serde(default)]
    pub label_weight: LabelWeightDto,
    #[serde(default)]
    pub label_style: LabelStyleDto,
    #[serde(default)]
    pub label_underline: bool,
    #[serde(default)]
    pub label_outline: bool,
    #[serde(default)]
    pub label_shadow: bool,
    pub label_size_pt: f64,
    pub carbon_labels: CarbonLabelsDto,
    pub display_unit: DisplayUnitDto,
}

/// Settings fields to change, or (in preferences) the fields new documents start from; omitted
/// fields keep their values (in preferences: the built-in defaults).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsEditDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond_length_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_font: Option<LabelFontDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_weight: Option<LabelWeightDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_style: Option<LabelStyleDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_outline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_shadow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carbon_labels: Option<CarbonLabelsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_unit: Option<DisplayUnitDto>,
}

/// Where a reset takes a document's settings from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSource {
    /// The built-in defaults.
    Defaults,
    /// The settings a new document would start from now: the defaults overlaid with the
    /// preferences' `new_documents`.
    Preferences,
}

/// Body of `settings/reset`. Added within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetSettingsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub to: SettingsSource,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
    #[serde(default)]
    pub selection_after: Option<SelectionDto>,
}

/// Body of `settings/import`: `source` names the document whose settings are applied. Added
/// within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSettingsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub source: String,
    #[serde(default)]
    pub selection_before: Option<SelectionDto>,
    #[serde(default)]
    pub selection_after: Option<SelectionDto>,
}

/// The domain checks the values' ranges.
pub(crate) fn to_settings_edit(edit: &SettingsEditDto) -> SettingsEdit {
    SettingsEdit {
        bond_length_pt: edit.bond_length_pt,
        line_width_pt: edit.line_width_pt,
        label_font: edit.label_font.map(|font| match font {
            LabelFontDto::SansSerif => LabelFont::SansSerif,
            LabelFontDto::Serif => LabelFont::Serif,
            LabelFontDto::Monospace => LabelFont::Monospace,
        }),
        label_weight: edit.label_weight.map(|weight| match weight {
            LabelWeightDto::Normal => LabelWeight::Normal,
            LabelWeightDto::Bold => LabelWeight::Bold,
        }),
        label_style: edit.label_style.map(|style| match style {
            LabelStyleDto::Normal => LabelStyle::Normal,
            LabelStyleDto::Italic => LabelStyle::Italic,
        }),
        label_underline: edit.label_underline,
        label_outline: edit.label_outline,
        label_shadow: edit.label_shadow,
        label_size_pt: edit.label_size_pt,
        carbon_labels: edit.carbon_labels.map(|labels| match labels {
            CarbonLabelsDto::Default => CarbonLabels::Default,
            CarbonLabelsDto::All => CarbonLabels::All,
        }),
        display_unit: edit.display_unit.map(|unit| match unit {
            DisplayUnitDto::Pt => DisplayUnit::Pt,
            DisplayUnitDto::Mm => DisplayUnit::Mm,
            DisplayUnitDto::Cm => DisplayUnit::Cm,
            DisplayUnitDto::In => DisplayUnit::In,
        }),
    }
}

pub(crate) fn settings_edit_view(edit: &SettingsEdit) -> SettingsEditDto {
    SettingsEditDto {
        bond_length_pt: edit.bond_length_pt,
        line_width_pt: edit.line_width_pt,
        label_font: edit.label_font.map(font_view),
        label_weight: edit.label_weight.map(weight_view),
        label_style: edit.label_style.map(style_view),
        label_underline: edit.label_underline,
        label_outline: edit.label_outline,
        label_shadow: edit.label_shadow,
        label_size_pt: edit.label_size_pt,
        carbon_labels: edit.carbon_labels.map(carbon_labels_view),
        display_unit: edit.display_unit.map(unit_view),
    }
}

pub(crate) fn settings_view(settings: &Settings) -> SettingsDto {
    SettingsDto {
        bond_length_pt: settings.bond_length_pt,
        line_width_pt: settings.line_width_pt,
        label_font: font_view(settings.label_font),
        label_weight: weight_view(settings.label_weight),
        label_style: style_view(settings.label_style),
        label_underline: settings.label_underline,
        label_outline: settings.label_outline,
        label_shadow: settings.label_shadow,
        label_size_pt: settings.label_size_pt,
        carbon_labels: carbon_labels_view(settings.carbon_labels),
        display_unit: unit_view(settings.display_unit),
    }
}

fn weight_view(weight: LabelWeight) -> LabelWeightDto {
    match weight {
        LabelWeight::Normal => LabelWeightDto::Normal,
        LabelWeight::Bold => LabelWeightDto::Bold,
    }
}

fn style_view(style: LabelStyle) -> LabelStyleDto {
    match style {
        LabelStyle::Normal => LabelStyleDto::Normal,
        LabelStyle::Italic => LabelStyleDto::Italic,
    }
}

pub(super) fn font_view(font: LabelFont) -> LabelFontDto {
    match font {
        LabelFont::SansSerif => LabelFontDto::SansSerif,
        LabelFont::Serif => LabelFontDto::Serif,
        LabelFont::Monospace => LabelFontDto::Monospace,
    }
}

fn carbon_labels_view(labels: CarbonLabels) -> CarbonLabelsDto {
    match labels {
        CarbonLabels::Default => CarbonLabelsDto::Default,
        CarbonLabels::All => CarbonLabelsDto::All,
    }
}

fn unit_view(unit: DisplayUnit) -> DisplayUnitDto {
    match unit {
        DisplayUnit::Pt => DisplayUnitDto::Pt,
        DisplayUnit::Mm => DisplayUnitDto::Mm,
        DisplayUnit::Cm => DisplayUnitDto::Cm,
        DisplayUnit::In => DisplayUnitDto::In,
    }
}
