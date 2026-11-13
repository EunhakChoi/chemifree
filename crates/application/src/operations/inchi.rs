//! Read-only Standard InChI generation, with an exact snapshot and loss consent. The request's
//! time limit and the host cancel are asked before the native call and again once the provider
//! holds its native lock; no compute slot is taken.

use chemifree_domain::stereo_group::GroupKind;
use serde::Deserialize;

use super::*;
use crate::inchi::{InchiErrorKind, InchiOutput, InchiProviderIdentity};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InchiParameters {
    document_id: String,
    #[serde(default)]
    accepted_losses: Option<String>,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused. It is asked
    /// before the native call and again once the provider holds its native lock, right before the
    /// call. The call is not interrupted, so a result that arrives after the limit is still
    /// returned.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

/// Identifiers, diagnostics and the immutable drawing and provider registration they used.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InchiResult {
    pub snapshot: DocumentResult,
    pub inchi: String,
    pub inchikey: String,
    pub aux_info: Option<String>,
    pub warnings: Vec<String>,
    pub native_code: i32,
    pub native_message: Option<String>,
    pub native_log: Option<String>,
    pub standard_semantics: &'static str,
    pub provider: InchiProviderIdentity,
    pub standard: bool,
}

fn valid_output(output: &InchiOutput) -> bool {
    let key = output.inchikey.as_bytes();
    output.inchi.starts_with("InChI=1S/")
        && output.inchi.len() > "InChI=1S/".len()
        && !output.inchi.bytes().any(|byte| byte.is_ascii_control())
        && key.len() == 27
        && key[14] == b'-'
        && key[25] == b'-'
        && key
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 14 || index == 25 || byte.is_ascii_uppercase())
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn generate_inchi(
        &self,
        known: &Known,
        parameters: InchiParameters,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        // The limit is read first, as the other read-only inspections read theirs: a limit above
        // the maximum is refused even when no provider is registered.
        let (budget, limit_ms) = match self.start_time_limit_or_refuse(
            known,
            parameters.time_limit_ms,
            cancel,
            crate::inchi::MODEL_ID,
        ) {
            Ok(started) => started,
            Err(reply) => return *reply,
        };
        let Some(provider) = &self.inchi_provider else {
            let mut details = Map::new();
            if let Some(error) = &self.inchi_provider_error {
                details.insert("reason_code".into(), error.code.into());
                details.insert("reason".into(), error.message.clone().into());
            }
            return known.unsupported(diagnostic(
                "inchi_unavailable",
                "The optional native InChI provider is unavailable. No identifier was generated.",
                details,
            ));
        };
        let (snapshot, graph, groups) = match self
            .documents
            .graph_and_groups_snapshot(&parameters.document_id)
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let revision = snapshot.document.revision;
        let identity = provider.identity();
        let mut context = Map::new();
        context.insert(
            "snapshot".into(),
            serde_json::to_value(&snapshot).expect("snapshot is serializable"),
        );
        context.insert(
            "provider".into(),
            serde_json::to_value(&identity).expect("provider identity is serializable"),
        );
        // Standard InChI cannot be presented as retaining relative or mixture group meaning.
        let unsupported: Vec<String> = groups
            .iter()
            .filter(|(_, group)| group.kind() != GroupKind::Absolute)
            .map(|(id, _)| id.to_string())
            .collect();
        if !unsupported.is_empty() {
            context.insert("groups".into(), serde_json::json!(unsupported));
            return known.reply(Outcome::Unsupported, Some(revision), None, vec![diagnostic(
                "inchi_enhanced_groups_unsupported",
                "Standard InChI generation does not support AND or OR group semantics; no groups were converted to absolute stereo and no identifier was generated.", context,
            )]);
        }
        if identity.name.is_empty() || identity.version.is_empty() || identity.build.is_empty() {
            return known.reply(
                Outcome::Error,
                Some(revision),
                None,
                vec![diagnostic(
                    "inchi_provider_invalid",
                    "The InChI provider did not identify its registration and build.",
                    context,
                )],
            );
        }
        // Asked before the call, and again by the provider once it holds its native lock, right
        // before the native call. Nothing is asked after the call begins: it cannot be interrupted,
        // so a call that runs past the limit still returns its result.
        let generated =
            self.compute_read_only(&budget, |stop| provider.generate(&graph, &groups, stop));
        let output = match generated {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                let outcome = match error.kind {
                    InchiErrorKind::Unavailable | InchiErrorKind::Unsupported => {
                        Outcome::Unsupported
                    }
                    InchiErrorKind::Invalid | InchiErrorKind::Limit | InchiErrorKind::Failed => {
                        Outcome::Error
                    }
                };
                return known.reply(
                    outcome,
                    Some(revision),
                    None,
                    vec![diagnostic(error.code, error.message, context)],
                );
            }
            Err(interrupted) => {
                return stopped_read_only(
                    known,
                    revision,
                    interrupted,
                    limit_ms,
                    crate::inchi::MODEL_ID,
                );
            }
        };
        if !valid_output(&output) {
            return known.reply(
                Outcome::Error,
                Some(revision),
                None,
                vec![diagnostic(
                    "inchi_provider_invalid",
                    "The provider returned invalid Standard InChI or InChIKey output.",
                    context,
                )],
            );
        }
        // Consent binds the scientific output and warning context. The raw native log is
        // diagnostic-only and may change between equivalent calls (for example timing).
        let consent_bytes = serde_json::to_vec(&(
            crate::inchi::MODEL_ID,
            &identity,
            &snapshot.document,
            &output.inchi,
            &output.inchikey,
            &output.aux_info,
            &output.warnings,
            output.native_code,
            &output.native_message,
        ))
        .expect("InChI consent context is serializable");
        if let Some(mut refusal) = interchange::export_consent_refusal(
            &output.losses,
            &consent_bytes,
            parameters.accepted_losses.as_deref(),
        ) {
            refusal.details.extend(context);
            refusal
                .details
                .insert("native_code".into(), output.native_code.into());
            refusal.details.insert(
                "native_message".into(),
                serde_json::to_value(&output.native_message)
                    .expect("native message is serializable"),
            );
            refusal.details.insert(
                "warnings".into(),
                serde_json::to_value(&output.warnings).expect("warnings are serializable"),
            );
            refusal.details.insert(
                "native_log".into(),
                serde_json::to_value(&output.native_log).expect("native log is serializable"),
            );
            return known.reply(Outcome::Error, Some(revision), None, vec![refusal]);
        }
        let outcome = if output.losses.is_empty() && output.warnings.is_empty() {
            Outcome::Ok
        } else {
            Outcome::Partial
        };
        let mut reply = known.reply(
            outcome,
            Some(revision),
            Some(OperationResult::Inchi(Box::new(InchiResult {
                snapshot: snapshot.into(),
                inchi: output.inchi,
                inchikey: output.inchikey,
                aux_info: output.aux_info,
                warnings: output.warnings,
                native_code: output.native_code,
                native_message: output.native_message,
                native_log: output.native_log,
                standard_semantics: crate::inchi::STANDARD_SEMANTICS,
                provider: identity,
                standard: true,
            }))),
            Vec::new(),
        );
        reply.loss_report = output.losses;
        reply.model_id = Some(crate::inchi::MODEL_ID.into());
        reply
    }
}
