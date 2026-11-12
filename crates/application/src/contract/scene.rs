//! Scene object, reference, arrangement, transform and caption shapes.

use chemifree_domain::scene::{
    AlignMode, CaptionAlignment, CaptionRun, CaptionScript, CaptionStyle, CaptionWeight,
    DistributionAxis, ObjectArrangement, SelectionTransform,
};
use serde::{Deserialize, Serialize};

use super::settings::{LabelFontDto, font_view};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicShapeDto {
    Line,
    Rectangle,
    Ellipse,
    Arrow,
    Polygon,
    Spline,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeleteModeDto {
    Subtree,
    ObjectOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceTargetDto {
    Object { id: String },
    Atom { id: String },
    Bond { id: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceDto {
    pub source: String,
    pub target: ReferenceTargetDto,
}

/// Geometry-only rigid arrangements of outermost selected scene objects.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrangementModeDto {
    AlignLeft,
    AlignHorizontalCenter,
    AlignRight,
    AlignTop,
    AlignVerticalCenter,
    AlignBottom,
    DistributeHorizontal,
    DistributeVertical,
}

impl From<ArrangementModeDto> for ObjectArrangement {
    fn from(mode: ArrangementModeDto) -> Self {
        match mode {
            ArrangementModeDto::AlignLeft => Self::Align(AlignMode::Left),
            ArrangementModeDto::AlignHorizontalCenter => Self::Align(AlignMode::HorizontalCenter),
            ArrangementModeDto::AlignRight => Self::Align(AlignMode::Right),
            ArrangementModeDto::AlignTop => Self::Align(AlignMode::Top),
            ArrangementModeDto::AlignVerticalCenter => Self::Align(AlignMode::VerticalCenter),
            ArrangementModeDto::AlignBottom => Self::Align(AlignMode::Bottom),
            ArrangementModeDto::DistributeHorizontal => {
                Self::Distribute(DistributionAxis::Horizontal)
            }
            ArrangementModeDto::DistributeVertical => Self::Distribute(DistributionAxis::Vertical),
        }
    }
}

/// Geometry-only transform about an explicit page-coordinate pivot.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionTransformDto {
    Rotate { degrees: f64 },
    // Empty struct variants keep deny_unknown_fields effective for reflection objects.
    ReflectHorizontal {},
    ReflectVertical {},
}

impl From<SelectionTransformDto> for SelectionTransform {
    fn from(transform: SelectionTransformDto) -> Self {
        match transform {
            SelectionTransformDto::Rotate { degrees } => Self::Rotate { degrees },
            SelectionTransformDto::ReflectHorizontal {} => Self::ReflectHorizontal,
            SelectionTransformDto::ReflectVertical {} => Self::ReflectVertical,
        }
    }
}

/// Fragment runs inherit document label style and vary only their baseline treatment.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentDisplayRunDto {
    pub text: String,
    pub script: CaptionScriptDto,
}

/// Physical alignment of a complete caption, independent of paragraph direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionAlignmentDto {
    #[default]
    Left,
    Center,
    Right,
}

impl From<CaptionAlignmentDto> for CaptionAlignment {
    fn from(value: CaptionAlignmentDto) -> Self {
        match value {
            CaptionAlignmentDto::Left => Self::Left,
            CaptionAlignmentDto::Center => Self::Center,
            CaptionAlignmentDto::Right => Self::Right,
        }
    }
}

impl From<CaptionAlignment> for CaptionAlignmentDto {
    fn from(value: CaptionAlignment) -> Self {
        match value {
            CaptionAlignment::Left => Self::Left,
            CaptionAlignment::Center => Self::Center,
            CaptionAlignment::Right => Self::Right,
        }
    }
}

/// A complete run value; no byte or UTF-16 substring indices cross the wire.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionRunDto {
    /// Omission means false; null and non-booleans are malformed.
    #[serde(default)]
    pub shadow: bool,
    /// Omission means false; null and non-booleans are malformed.
    #[serde(default)]
    pub outline: bool,
    /// Omission means false; null and non-booleans are malformed.
    #[serde(default)]
    pub underline: bool,
    /// Null or omission follows the theme foreground; overrides are opaque sRGB #RRGGBB.
    #[serde(default)]
    pub color: Option<String>,
    /// Null or omission dynamically follows the document label size (finite 4–72 pt).
    #[serde(default)]
    pub size_pt: Option<f64>,
    /// Null or omission follows the document label font.
    #[serde(default)]
    pub font: Option<LabelFontDto>,
    pub text: String,
    pub script: CaptionScriptDto,
    #[serde(default)]
    pub style: CaptionStyleDto,
    #[serde(default)]
    pub weight: CaptionWeightDto,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionScriptDto {
    Baseline,
    Subscript,
    Superscript,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionWeightDto {
    #[default]
    Normal,
    Bold,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionStyleDto {
    #[default]
    Normal,
    Italic,
}

impl From<&CaptionRun> for CaptionRunDto {
    fn from(run: &CaptionRun) -> Self {
        Self {
            shadow: run.shadow,
            outline: run.outline,
            underline: run.underline,
            color: run.color.clone(),
            size_pt: run.size_pt,
            font: run.font.map(font_view),
            text: run.text.clone(),
            style: match run.style {
                CaptionStyle::Normal => CaptionStyleDto::Normal,
                CaptionStyle::Italic => CaptionStyleDto::Italic,
            },
            weight: match run.weight {
                CaptionWeight::Normal => CaptionWeightDto::Normal,
                CaptionWeight::Bold => CaptionWeightDto::Bold,
            },
            script: match run.script {
                CaptionScript::Baseline => CaptionScriptDto::Baseline,
                CaptionScript::Subscript => CaptionScriptDto::Subscript,
                CaptionScript::Superscript => CaptionScriptDto::Superscript,
            },
        }
    }
}
