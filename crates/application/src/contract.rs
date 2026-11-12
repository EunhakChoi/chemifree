//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

mod chemistry;
mod command;
mod convert;
mod parse;
mod request;
mod response;
mod scene;
mod settings;
mod stereo;
mod view;

pub use chemistry::{
    BondOrderDto, BondQueryDto, BondToDto, HydrogensDto, QueryBondOrderDto, QueryDto, RadicalDto,
    query_label,
};
pub use command::{CommandDto, RingDirectionDto, TransactionRequest};
pub(crate) use convert::to_commands;
pub(crate) use parse::{
    attachment_points, bond_order, exact_selection, instance_id, point, query_atom, to_hydrogens,
    to_selection,
};
pub use request::{
    CloseRequest, CreateDocumentRequest, DraftRequest, ListQuery, ListSort, PointDto,
    RecoverAsCopyRequest, RenameRequest, RestoreRequest, RevisionRequest, SaveAsRequest,
    SelectionDto, TransactionSelections,
};
pub use response::{
    CloseResponse, CopyResponse, CopySource, DeleteResponse, DocumentListResponse,
    DocumentResponse, DocumentSummary, DraftDiscardResponse, DraftFailureView, RecoveryState,
    RecoveryView, RestoreResponse, SessionView, TrashListResponse, TrashedSummary,
    UnreadableDocument,
};
pub use scene::{
    ArrangementModeDto, CaptionAlignmentDto, CaptionRunDto, CaptionScriptDto, CaptionStyleDto,
    CaptionWeightDto, DeleteModeDto, GraphicShapeDto, ReferenceDto, ReferenceTargetDto,
    SelectionTransformDto,
};
pub use settings::{
    CarbonLabelsDto, DisplayUnitDto, ImportSettingsRequest, LabelFontDto, LabelStyleDto,
    LabelWeightDto, ResetSettingsRequest, SettingsDto, SettingsEditDto, SettingsSource,
};
pub(crate) use settings::{settings_edit_view, settings_view, to_settings_edit};
pub(crate) use stereo::slot_name;
pub use stereo::{
    AtomConfigurationDto, AtomStereoView, BondConfigurationDto, BondEndDto, BondStereoView,
    MarkDto, RelationDto, StereoGroupKindDto, StereoGroupView, StereoView, TurnDto, center_dto,
    double_bond_dto,
};
pub use view::{
    AbbreviationView, AtomKindView, AtomPositionView, AtomView, BondView, DocumentView,
    InstanceView, MoleculeView, PageView, PositionView, SceneObjectView, SceneView,
};
