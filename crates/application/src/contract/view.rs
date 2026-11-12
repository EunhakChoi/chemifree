//! The document snapshot: graph, scene, page and placement views.

use serde::Serialize;

#[cfg(doc)]
use super::chemistry::query_label;
use super::chemistry::{BondOrderDto, BondQueryDto, HydrogensDto, QueryDto, RadicalDto};
use super::request::SelectionDto;
use super::scene::{
    CaptionAlignmentDto, CaptionRunDto, FragmentDisplayRunDto, GraphicShapeDto, ReferenceDto,
};
use super::settings::SettingsDto;
use super::stereo::{MarkDto, StereoView};

/// `atoms` and `bonds` are the chemical graph, `molecule`; `scene` places it on the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentView {
    /// Plain display text never defines chemistry; every member remains in the graph.
    pub fragment_displays: Vec<FragmentDisplayView>,
    /// Et is display metadata; all graph IDs and coordinates remain present.
    pub abbreviations: Vec<AbbreviationView>,
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atoms: Vec<AtomView>,
    pub bonds: Vec<BondView>,
    pub molecule: MoleculeView,
    pub scene: SceneView,
    pub selection: SelectionDto,
    pub settings: SettingsDto,
    /// Stated stereo configurations. Added within v1; always present, with empty lists when no
    /// configuration is stated. No record means "unspecified": it says nothing about whether an
    /// atom or bond is stereogenic.
    pub stereo: StereoView,
}

/// A bounded explicit fragment contraction, independent of Et metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FragmentDisplayView {
    pub instance: String,
    pub anchor: String,
    pub members: Vec<String>,
    pub text: String,
    pub runs: Vec<FragmentDisplayRunDto>,
}

/// The two real graph atoms represented by a per-placement Et label.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AbbreviationView {
    pub display_label: String,
    pub instance: String,
    pub anchor: String,
    pub terminal: String,
}

impl DocumentView {
    /// Whether the drawing holds any stereo mark or stated configuration, of any kind
    /// (including wavy, crossed, and `unknown`). Operations that ignore stereo say so when this
    /// is true.
    pub fn holds_stereo(&self) -> bool {
        !self.stereo.atoms.is_empty()
            || !self.stereo.bonds.is_empty()
            || self.bonds.iter().any(|bond| bond.mark.is_some())
    }
}

/// Positions are scene points; x grows right and y grows down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomView {
    pub id: String,
    /// The element symbol, or for a query atom its label (see [`query_label`]), which is not
    /// an element symbol: `kind` says which.
    pub element: String,
    pub charge: i8,
    pub isotope: Option<u16>,
    /// The atom's position in the first scene instance, kept for v1 clients.
    pub position: PositionView,
    /// `element` or `query`. This and the fields below were added within v1.
    pub kind: AtomKindView,
    /// The query a query atom stands for; null for an element.
    pub query: Option<QueryDto>,
    /// `{"mode": "inferred"}` has no stored count; inspection may derive one separately.
    pub hydrogens: HydrogensDto,
    pub radical: RadicalDto,
    /// Ascending, 1-4; empty when there are none.
    pub attachment_points: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomKindView {
    Element,
    Query,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MoleculeView {
    pub id: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneView {
    pub pages: Vec<PageView>,
    /// In order; the first one's positions are `atoms[].position`.
    pub instances: Vec<InstanceView>,
    /// Page roots and all objects in depth-first drawing order.
    pub roots: Vec<String>,
    pub objects: Vec<SceneObjectView>,
    pub references: Vec<ReferenceDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneObjectView {
    Caption {
        id: String,
        parent: Option<String>,
        anchor: PositionView,
        alignment: CaptionAlignmentDto,
        runs: Vec<CaptionRunDto>,
    },
    Instance {
        id: String,
        parent: Option<String>,
        molecule: &'static str,
    },
    Group {
        id: String,
        parent: Option<String>,
        children: Vec<String>,
    },
    Image {
        id: String,
        parent: Option<String>,
        mime_type: &'static str,
        data_base64: String,
        origin: PositionView,
        size_pt: PositionView,
        pixel_width: u32,
        pixel_height: u32,
    },
    Graphic {
        id: String,
        parent: Option<String>,
        line_width_pt: Option<f64>,
        shape: GraphicShapeDto,
        points: Vec<PositionView>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageView {
    pub id: &'static str,
}

/// A molecule scene instance: the molecule drawn on a page, one position per atom.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InstanceView {
    pub id: String,
    pub page: &'static str,
    pub molecule: &'static str,
    pub positions: Vec<AtomPositionView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomPositionView {
    pub atom: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PositionView {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondView {
    pub id: String,
    /// The donor when `order` is `dative`.
    pub begin: String,
    pub end: String,
    pub order: BondOrderDto,
    /// What a bond of order `query` stands for; null otherwise. Added within v1.
    pub query: Option<BondQueryDto>,
    /// How the bond is drawn; null for a plain bond. Added within v1. A drawing fact only: it
    /// states no configuration, and `null` means "plain", not "non-stereogenic".
    pub mark: Option<MarkDto>,
}
