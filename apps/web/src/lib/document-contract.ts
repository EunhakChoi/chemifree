/**
 * Hand-maintained mirror of the v1 document contract in crates/application/src/contract/
 * (re-exported by contract.rs).
 * The code lives in ./document-contract/; this file is the only entry point for importers.
 */

export {
  MAX_STATED_HYDROGENS, ATTACHMENT_POINTS, isGraphicShape, isSceneGraphicShape,
  MIN_POLYGON_VERTICES, MAX_POLYGON_VERTICES, graphicPointCountValid, DEFAULT_SETTINGS,
  SETTING_RANGES, LABEL_STYLES, LABEL_WEIGHTS, LABEL_FONTS, CARBON_LABELS, DISPLAY_UNITS,
  fragmentDisplayRuns,
} from '@/lib/document-contract/model'
export type {
  BondOrder, QueryBondOrder, Position, QueryAtom, BondQuery, Hydrogens, Radical, AtomView,
  InstancePosition, InstanceView, CaptionAlignment, CaptionRun, GraphicShape, SceneGraphicShape,
  SceneObject, ReferenceTarget, SceneReference, SelectionView, SelectionSnapshots, SceneView,
  BondView, LabelFont, LabelWeight, LabelStyle, CarbonLabels, DisplayUnit, DocumentSettings,
  SettingsEdit, NumericSetting, FragmentDisplayRun, FragmentDisplayView, AbbreviationView,
  DocumentView,
} from '@/lib/document-contract/model'
export { onlyKeys } from '@/lib/document-contract/guards'
export { ApiError, ComputeStopped } from '@/lib/document-contract/responses'
export type {
  SessionView, DraftFailureView, RecoveryState, RecoveryView, DocumentResponse, CopyResponse,
  DocumentSummary, ListSort, ListQuery, TrashedSummary, TrashListResponse, DocumentListResponse,
  StorageReason, ErrorDetails,
} from '@/lib/document-contract/responses'
export {
  SETTING_FIELDS, isDocumentSettings, isCaptionColor, validCaptionRuns, validDisplayLabel,
  validFragmentDisplayRuns, isDocumentResponse,
} from '@/lib/document-contract/readers'
export {
  SubstructureRefusal, sameInspectionSnapshot, samePropertySnapshot, settingsOf, sameConversionSnapshot,
  sameDrawnContent, conversionSnapshotMatcher,
} from '@/lib/document-contract/inspection'
export type {
  ValenceAtom, ValenceInspection, AromaticitySystem, AromaticityInspection, StereoInspection, CipInspection,
  CipParameters, IdentityMode, CanonicalIdentity, IdentityInspection, MatchPair, SubstructureMapping,
  SubstructureLimits, SubstructureSearch, PropertiesScope, PropertyKind, OmittedIsotopeAbundance, PropertyMass,
  PropertyComponent, PropertiesInspection,
} from '@/lib/document-contract/inspection'
export {
  MAX_NATIVE_CLIPBOARD_BYTES, MAX_CLIPBOARD_SMILES_BYTES, MAX_CLIPBOARD_MOL_BYTES, placements,
} from '@/lib/document-contract/editing'
export type {
  RingSize, RingPattern, RingInsertionAttempt, NativeClipboardCopy, NativeClipboardPaste,
  NativeSceneClipboardCopy, NativeSceneClipboardPaste, NativeSceneClipboardPasteAttempt, NativeClipboardIdMapping,
  TemplateParameters, TemplateReplacementParameters, TemplateReplacementOffer, TemplateInspection,
  TemplateAttachmentOffer, NativeClipboardPasteAttempt, NativeClipboardCutAttempt, NativeClipboardMolImportAttempt,
  SmilesClipboardImportAttempt, AtomRef, NewAtomPlacement, AtomStates, BondKind, ArrangementMode, SelectionTransform,
  Command, Placement,
} from '@/lib/document-contract/editing'
export {
  ConsentRequired, describeSpan,
} from '@/lib/document-contract/interchange'
export type {
  InterchangeLoss, InterchangeChoice, InterchangeFormat, InterchangeExport, SvgViewBox, SvgExport, InterchangeInput,
  InterchangePreview, InterchangeMapping, CtFormat, CtLoss, CtPreview, CtImport, SmilesStyle, SmilesSpan,
  SmilesPreview, SmilesImport, SmilesExport,
} from '@/lib/document-contract/interchange'
export {
  THEMES, isPreferencesResponse, outcomeOpen,
} from '@/lib/document-contract/transport'
export type {
  Theme, ComputationSetting, PreferencesResponse, PreferencesChange, SettingsSource,
} from '@/lib/document-contract/transport'
export { DocumentClient } from '@/lib/document-contract/client'
