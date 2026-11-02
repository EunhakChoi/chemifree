//! The one opaque-content policy shared by the CDXML and CDX write paths.
//!
//! Uninterpreted content may hold references or namespace-sensitive text, so a changed
//! document that has it, or originally had it, is rewritten only after an explicit discard,
//! and every discard is reported. A document equal to its re-parsed original is returned
//! as the original bytes, also when a discard was requested but there is nothing to discard.

use chemifree_application::interchange_codec::InterchangeError;
use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::interchange::{Content, Document, Object, PropertyKind};

/// What a format-specific reader found when it re-parsed the caller's original.
pub(crate) struct ParsedOriginal<'a> {
    pub document: &'a Document,
    /// Material outside the object tree (CDXML comments before the root, for example)
    /// that can survive only in the original bytes.
    pub unrewritable_outside: bool,
}

pub(crate) enum Plan {
    /// Return the original bytes untouched.
    KeepOriginal { losses: Vec<Loss> },
    /// Serialize afresh. `discarded` is the submitted tree after an explicit discard.
    Rewrite {
        discarded: Option<Document>,
        losses: Vec<Loss>,
    },
}

/// Opaque entries in a tree; each opaque subtree counts once.
pub(crate) fn opaque_count(object: &Object) -> usize {
    let properties = object
        .properties
        .iter()
        .filter(|property| {
            matches!(
                property.kind,
                PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
            )
        })
        .count();
    properties
        + object
            .children
            .iter()
            .map(|child| match child {
                Content::Object(child) if child.kind.is_opaque() => 1,
                Content::Object(child) => opaque_count(child),
                _ => 0,
            })
            .sum::<usize>()
}

pub(crate) fn opaque_loss(
    disposition: LossDisposition,
    original: Option<usize>,
    submitted: Option<usize>,
    outside: bool,
) -> Loss {
    let count = |value: Option<usize>| {
        value.map_or_else(|| "not supplied".into(), |value| value.to_string())
    };
    Loss {
        consent: LossConsent::NotRequired,
        feature: "opaque_extensions".into(),
        source_path: "document".into(),
        disposition,
        reason: format!(
            "Original opaque entries: {}; submitted opaque entries: {}; each subtree counts once and the sets may overlap. Outside-root material: {outside}. Unknown references cannot be verified or safely rewritten.",
            count(original),
            count(submitted)
        ),
        semantic_impact: SemanticImpact::Unknown,
    }
}

/// The loss list for a read, or for an unchanged document returned as its original.
pub(crate) fn retained_losses(count: usize, outside: bool) -> Vec<Loss> {
    if count > 0 || outside {
        vec![opaque_loss(
            LossDisposition::Retained,
            Some(count),
            None,
            outside,
        )]
    } else {
        Vec::new()
    }
}

/// Decides how `submitted` may be written given the re-parsed `original`.
pub(crate) fn plan(
    original: Option<ParsedOriginal<'_>>,
    submitted: &Document,
    discard_opaque: bool,
) -> Result<Plan, InterchangeError> {
    let outside = original
        .as_ref()
        .is_some_and(|original| original.unrewritable_outside);
    if let Some(original) = &original
        && original.document.root() == submitted.root()
    {
        let count = opaque_count(submitted.root());
        // An explicit discard changes nothing when there is nothing to discard, so the
        // unchanged original is still returned as it was.
        if !discard_opaque || (count == 0 && !outside) {
            return Ok(Plan::KeepOriginal {
                losses: retained_losses(count, outside),
            });
        }
    }
    let source_count = original
        .as_ref()
        .map_or(0, |original| opaque_count(original.document.root()));
    let submitted_count = opaque_count(submitted.root());
    let any = source_count > 0 || submitted_count > 0 || outside;
    if any && !discard_opaque {
        return Err(InterchangeError {
            code: "unsafe_unknown_rewrite",
            message: "An edited document contains or originally contained opaque content. Explicitly discard it before rewriting.".into(),
            line: None,
            column: None,
            offset: None,
        });
    }
    if discard_opaque && any {
        let mut discarded = submitted.clone();
        discarded
            .discard_opaque()
            .map_err(|error| InterchangeError {
                code: "invalid_document",
                message: format!(
                    "Discarding opaque content would leave an invalid document: {error}"
                ),
                line: None,
                column: None,
                offset: None,
            })?;
        return Ok(Plan::Rewrite {
            discarded: Some(discarded),
            losses: vec![opaque_loss(
                LossDisposition::Dropped,
                original.as_ref().map(|_| source_count),
                Some(submitted_count),
                outside,
            )],
        });
    }
    Ok(Plan::Rewrite {
        discarded: None,
        losses: Vec::new(),
    })
}
