//! Document replies: the document response, session and recovery state, and list/trash replies.

use chemifree_domain::document::{BondOrder, Document, MOLECULE_ID, PAGE_ID};
use chemifree_domain::scene::{
    CaptionScript, GraphicShape, ReferenceTarget, SceneObject, SceneReference,
};
use chemifree_domain::states::AtomKind;
use serde::Serialize;

use crate::API_VERSION;
use crate::operations::Loss;

use super::chemistry::query_label;
use super::request::SelectionDto;
use super::scene::{
    CaptionRunDto, CaptionScriptDto, FragmentDisplayRunDto, GraphicShapeDto, ReferenceDto,
    ReferenceTargetDto,
};
use super::settings::settings_view;
use super::stereo::{MarkDto, StereoView};
use super::view::{
    AbbreviationView, AtomKindView, AtomPositionView, AtomView, BondView, DocumentView,
    FragmentDisplayView, InstanceView, MoleculeView, PageView, PositionView, SceneObjectView,
    SceneView,
};

fn reference_view(reference: SceneReference) -> ReferenceDto {
    let target = match reference.target {
        ReferenceTarget::Object(id) => ReferenceTargetDto::Object { id: id.to_string() },
        ReferenceTarget::Atom(id) => ReferenceTargetDto::Atom { id: id.to_string() },
        ReferenceTarget::Bond(id) => ReferenceTargetDto::Bond { id: id.to_string() },
    };
    ReferenceDto {
        source: reference.source.to_string(),
        target,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResponse {
    pub api_version: &'static str,
    pub document: DocumentView,
    pub session: SessionView,
    /// A recovery draft left by an earlier session of this document, if any.
    pub recovery: Option<RecoveryView>,
    /// What this request cleared of stereo because the edit changed what it was about, as
    /// loss entries; an empty list for every reply but a transaction that cleared something.
    /// Added within v1; always present. It describes this request only: `get`, undo, redo and
    /// later replies never repeat it, and a replayed `request_id` returns it again.
    pub notes: Vec<Loss>,
}

/// An abandoned recovery draft: unsaved work of a session whose process ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecoveryView {
    pub state: RecoveryState,
    /// The draft's revision, in the numbering of the process that wrote it.
    pub revision: Option<u64>,
    /// The stored revision the draft's session started from.
    pub base_revision: Option<u64>,
    /// Milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
    /// Why the draft cannot be recovered in place, when it cannot.
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// Storage still holds the draft's base: it can be recovered in place.
    Recoverable,
    /// Another writer saved since the draft's base: it can be recovered as a copy.
    Outdated,
    /// The draft cannot be read; it can only be discarded.
    Unreadable,
    /// Another running process owns it.
    InUse,
}

/// State of the in-memory editing session that holds the working copy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionView {
    /// Revision last committed to storage by this session.
    pub saved_revision: u64,
    pub unsaved_changes: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    /// The revision this session's recovery draft holds, or `null` when it has none.
    pub draft_revision: Option<u64>,
    /// The service's latest failed attempt to draft unsaved work this draft does not hold,
    /// while it keeps retrying; `null` otherwise.
    pub draft_failure: Option<DraftFailureView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftFailureView {
    /// Failed attempts in a row.
    pub attempts: u32,
    /// Milliseconds until the service tries again.
    pub retry_in_ms: u64,
    /// The latest attempt's error code, as in the v1 error envelope.
    pub code: &'static str,
    /// With `storage_failed`, why storage failed, as in the v1 error envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub message: String,
}

impl DocumentResponse {
    pub(crate) fn new(
        document: &Document,
        saved_revision: u64,
        draft_revision: Option<u64>,
    ) -> Self {
        let first = document.first_instance();
        Self {
            api_version: API_VERSION,
            document: DocumentView {
                fragment_displays: document
                    .fragment_displays()
                    .map(|(key, display)| FragmentDisplayView {
                        instance: key.instance.to_string(),
                        anchor: key.atom.to_string(),
                        members: display.members.iter().map(ToString::to_string).collect(),
                        text: display.text(),
                        runs: display
                            .runs
                            .iter()
                            .map(|run| FragmentDisplayRunDto {
                                text: run.text.clone(),
                                script: match run.script {
                                    CaptionScript::Baseline => CaptionScriptDto::Baseline,
                                    CaptionScript::Subscript => CaptionScriptDto::Subscript,
                                    CaptionScript::Superscript => CaptionScriptDto::Superscript,
                                },
                            })
                            .collect(),
                    })
                    .collect(),
                abbreviations: document
                    .abbreviations()
                    .map(|(key, terminal)| AbbreviationView {
                        display_label: document.abbreviation_label(key).to_owned(),
                        instance: key.instance.to_string(),
                        anchor: key.atom.to_string(),
                        terminal: terminal.to_string(),
                    })
                    .collect(),
                id: document.id().to_string(),
                title: document.title().to_owned(),
                revision: document.revision(),
                atoms: document
                    .atoms()
                    .map(|(id, atom)| {
                        let position = document
                            .position(first, id)
                            .expect("every atom has a position in every scene instance");
                        let (kind, element, query) = match &atom.kind {
                            AtomKind::Element(element) => {
                                (AtomKindView::Element, element.symbol().to_owned(), None)
                            }
                            AtomKind::Query(query) => {
                                (AtomKindView::Query, query_label(query), Some(query.into()))
                            }
                        };
                        AtomView {
                            id: id.to_string(),
                            element,
                            charge: atom.charge,
                            isotope: atom.isotope,
                            position: PositionView {
                                x: position.x(),
                                y: position.y(),
                            },
                            kind,
                            query,
                            hydrogens: atom.hydrogens.into(),
                            radical: atom.radical.into(),
                            attachment_points: atom.attachment_points.values().collect(),
                        }
                    })
                    .collect(),
                bonds: document
                    .bonds()
                    .map(|(id, bond)| BondView {
                        id: id.to_string(),
                        begin: bond.begin.to_string(),
                        end: bond.end.to_string(),
                        order: (&bond.order).into(),
                        query: match &bond.order {
                            BondOrder::Query(query) => Some(query.into()),
                            _ => None,
                        },
                        mark: bond.mark.map(MarkDto::from),
                    })
                    .collect(),
                molecule: MoleculeView { id: MOLECULE_ID },
                scene: SceneView {
                    pages: vec![PageView { id: PAGE_ID }],
                    instances: document
                        .instances()
                        .map(|instance| InstanceView {
                            id: instance.to_string(),
                            page: PAGE_ID,
                            molecule: MOLECULE_ID,
                            positions: document
                                .positions(instance)
                                .map(|(atom, point)| AtomPositionView {
                                    atom: atom.to_string(),
                                    x: point.x(),
                                    y: point.y(),
                                })
                                .collect(),
                        })
                        .collect(),
                    roots: document.roots().iter().map(ToString::to_string).collect(),
                    objects: document
                        .scene_objects()
                        .into_iter()
                        .map(|(id, parent, object)| {
                            let id = id.to_string();
                            let parent = parent.map(|parent| parent.to_string());
                            match object {
                                SceneObject::Caption(caption) => SceneObjectView::Caption {
                                    id,
                                    parent,
                                    anchor: PositionView {
                                        x: caption.anchor.x(),
                                        y: caption.anchor.y(),
                                    },
                                    alignment: caption.alignment.into(),
                                    runs: caption.runs.iter().map(CaptionRunDto::from).collect(),
                                },
                                SceneObject::Instance => SceneObjectView::Instance {
                                    id,
                                    parent,
                                    molecule: MOLECULE_ID,
                                },
                                SceneObject::Group { children } => SceneObjectView::Group {
                                    id,
                                    parent,
                                    children: children.iter().map(ToString::to_string).collect(),
                                },
                                SceneObject::Image(image) => SceneObjectView::Image {
                                    id,
                                    parent,
                                    mime_type: "image/png",
                                    data_base64: image.asset.data_base64().to_owned(),
                                    origin: PositionView {
                                        x: image.origin.x(),
                                        y: image.origin.y(),
                                    },
                                    size_pt: PositionView {
                                        x: image.size_pt.x(),
                                        y: image.size_pt.y(),
                                    },
                                    pixel_width: image.asset.width(),
                                    pixel_height: image.asset.height(),
                                },
                                SceneObject::Graphic(graphic) => SceneObjectView::Graphic {
                                    id,
                                    parent,
                                    line_width_pt: graphic.line_width_pt,
                                    shape: match graphic.shape {
                                        GraphicShape::Line => GraphicShapeDto::Line,
                                        GraphicShape::Rectangle => GraphicShapeDto::Rectangle,
                                        GraphicShape::Ellipse => GraphicShapeDto::Ellipse,
                                        GraphicShape::Arrow => GraphicShapeDto::Arrow,
                                        GraphicShape::Polygon => GraphicShapeDto::Polygon,
                                        GraphicShape::Spline => GraphicShapeDto::Spline,
                                    },
                                    points: graphic
                                        .points
                                        .iter()
                                        .map(|point| PositionView {
                                            x: point.x(),
                                            y: point.y(),
                                        })
                                        .collect(),
                                },
                            }
                        })
                        .collect(),
                    references: document.references().map(reference_view).collect(),
                },
                selection: SelectionDto {
                    objects: document
                        .selection()
                        .objects
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    atoms: document
                        .selection()
                        .atoms
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    bonds: document
                        .selection()
                        .bonds
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                },
                settings: settings_view(document.settings()),
                stereo: StereoView::try_new(document)
                    .expect("a document's stereo records are kept with the kind of their key"),
            },
            session: SessionView {
                saved_revision,
                unsaved_changes: saved_revision != document.revision(),
                undo_depth: document.undo_depth(),
                redo_depth: document.redo_depth(),
                draft_revision,
                draft_failure: None,
            },
            recovery: None,
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentListResponse {
    pub api_version: &'static str,
    pub documents: Vec<DocumentSummary>,
    /// Stored entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
    /// True when the cap of MAX_LISTED_DOCUMENTS left stored documents unread, so the list is
    /// incomplete and a title search cannot find the unread ones.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    pub open: bool,
    pub unsaved_changes: bool,
    pub recovery: Option<RecoveryView>,
    /// When storage last committed the document, in milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnreadableDocument {
    pub id: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CloseResponse {
    pub api_version: &'static str,
    pub id: String,
    /// True when the session had unsaved changes and they were discarded.
    pub discarded_changes: bool,
}

/// A document saved as a new, independent document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopyResponse {
    /// The copy, now open in its own session.
    #[serde(flatten)]
    pub copy: DocumentResponse,
    pub source: CopySource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopySource {
    pub id: String,
    /// True when the source was open when the copy was made and is no longer open when the
    /// reply is recorded. A recovered-draft copy only evicts a clean source as a last resort;
    /// another request may also evict it while draft removal is in progress.
    pub closed: bool,
    /// True when that session had unsaved changes, which are now only in the copy.
    pub discarded_changes: bool,
}

/// A document moved to the trash.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeleteResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

/// A trashed document moved back to the stored documents.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RestoreResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashListResponse {
    pub api_version: &'static str,
    /// Most recently trashed first.
    pub documents: Vec<TrashedSummary>,
    /// Trashed entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
    /// True when the cap of MAX_LISTED_DOCUMENTS left trashed documents unread, so the list
    /// is incomplete.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashedSummary {
    pub id: String,
    pub title: String,
    /// The stored revision it was trashed at.
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    /// Milliseconds since the Unix epoch, when known.
    pub deleted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftDiscardResponse {
    pub api_version: &'static str,
    pub id: String,
}
