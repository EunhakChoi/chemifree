//! Formula and mass arithmetic over explicitly supplied, versioned isotope data.
//!
//! This module contains no element masses or abundances. A caller must supply a complete,
//! selected data generation; incomplete inputs and resource limits are errors, not estimates.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{AtomId, Document};
use crate::element::Element;
use crate::scene::{SceneObject, Selection};
use crate::states::Hydrogens;

pub const MAX_PEAKS: usize = 4096;
pub const MAX_DISTRIBUTION_WORK: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MassError {
    MissingElement,
    MissingIsotope,
    MissingAverage,
    MissingAbundance,
    InvalidData,
    ResourceLimit,
    ProbabilityUnderflow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Isotope {
    pub number: u16,
    pub mass: f64,
    /// A fraction of one, or None when the supplied data did not report abundance.
    pub abundance: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementMass {
    pub average: Option<f64>,
    pub average_basis: AverageBasis,
    pub selected_isotope: u16,
    pub selection_basis: ExactBasis,
    pub isotopes: Vec<Isotope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExactBasis {
    PinnedProfileSelection,
    MaximumReportedFraction,
    ExplicitIsotope,
    Unavailable,
}

impl ExactBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PinnedProfileSelection => "pinned_profile_selection",
            Self::MaximumReportedFraction => "maximum_reported_fraction",
            Self::ExplicitIsotope => "explicit_isotope",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AverageBasis {
    ProfileNaturalWeight,
    ReportedIsotopeComposition,
    ReportedStandardAtomicWeight,
    Unavailable,
}

impl AverageBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProfileNaturalWeight => "profile_natural_weight",
            Self::ReportedIsotopeComposition => "reported_isotope_composition",
            Self::ReportedStandardAtomicWeight => "reported_standard_atomic_weight",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Source selection and version belong to the calling application. The charge correction is
/// a signed mass in u per unit of elementary charge, explicitly supplied by that profile.
#[derive(Clone, Debug, PartialEq)]
pub struct MassTable {
    pub elements: BTreeMap<Element, ElementMass>,
    pub charge_correction: f64,
    pub missing_abundance: MissingAbundancePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingAbundancePolicy {
    Reject,
    /// A conditional distribution over only the isotope fractions actually reported.
    ConditionOnReported,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Composition {
    pub species: BTreeMap<(Element, Option<u16>), u32>,
    pub charge: i32,
}

/// Which chemical atoms contribute. Scene instances are placements of one graph, not copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaScope {
    Molecule,
    Selection,
    Components,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaError {
    NoAtoms,
    MissingSelection,
    InvalidSelection,
    UnknownAtom(AtomId),
    QueryAtom(AtomId),
    UnknownHydrogens(AtomId),
    UnknownAttachment(AtomId),
    CountOverflow,
}

/// Select atom groups without changing the document. Selected bonds contribute both endpoints;
/// a selected molecule instance or a containing group contributes the shared graph once.
/// Graphics and scene references are not chemical matter. Components follow graph connectivity.
pub fn atom_groups(
    document: &Document,
    scope: FormulaScope,
    selection: Option<&Selection>,
) -> Result<Vec<Vec<AtomId>>, FormulaError> {
    let all: BTreeSet<_> = document.atoms().map(|(id, _)| id).collect();
    let mut chosen = match scope {
        FormulaScope::Molecule | FormulaScope::Components => all.clone(),
        FormulaScope::Selection => {
            // The editor's current selection can differ from the last history snapshot.
            let selection = selection.ok_or(FormulaError::MissingSelection)?;
            let bonds: BTreeSet<_> = document.bonds().map(|(id, _)| id).collect();
            if !selection.atoms.is_subset(&all)
                || !selection.bonds.is_subset(&bonds)
                || selection
                    .objects
                    .iter()
                    .any(|&id| document.object(id).is_none())
            {
                return Err(FormulaError::InvalidSelection);
            }
            let mut chosen = selection.atoms.clone();
            for (id, bond) in document.bonds() {
                if selection.bonds.contains(&id) {
                    chosen.insert(bond.begin);
                    chosen.insert(bond.end);
                }
            }
            let mut pending: Vec<_> = selection.objects.iter().copied().collect();
            while let Some(id) = pending.pop() {
                match document.object(id) {
                    Some(SceneObject::Instance) => chosen.extend(&all),
                    Some(SceneObject::Group { children }) => pending.extend(children),
                    Some(SceneObject::Graphic(_)) | None => (),
                }
            }
            chosen
        }
    };
    if chosen.is_empty() {
        return Err(FormulaError::NoAtoms);
    }
    if scope != FormulaScope::Components {
        return Ok(vec![chosen.into_iter().collect()]);
    }
    let mut adjacency: BTreeMap<_, Vec<_>> = chosen.iter().map(|&id| (id, Vec::new())).collect();
    for (_, bond) in document.bonds() {
        adjacency
            .get_mut(&bond.begin)
            .expect("validated endpoint")
            .push(bond.end);
        adjacency
            .get_mut(&bond.end)
            .expect("validated endpoint")
            .push(bond.begin);
    }
    let mut groups = Vec::new();
    while let Some(&first) = chosen.first() {
        let mut group = Vec::new();
        let mut pending = vec![first];
        chosen.remove(&first);
        while let Some(id) = pending.pop() {
            group.push(id);
            for neighbor in &adjacency[&id] {
                if chosen.remove(neighbor) {
                    pending.push(*neighbor);
                }
            }
        }
        group.sort();
        groups.push(group);
    }
    Ok(groups)
}

/// Build a formula for named atoms. Inferred non-vertex H counts must come from a declared
/// model run over the whole graph, so selecting one end of a bond cannot create extra H.
/// Specified H is recorded directly; an explicit H vertex is counted only if selected.
pub fn composition(
    document: &Document,
    atoms: &[AtomId],
    inferred_hydrogens: &BTreeMap<AtomId, u16>,
) -> Result<Composition, FormulaError> {
    let available: BTreeMap<_, _> = document.atoms().collect();
    let ids: BTreeSet<_> = atoms.iter().copied().collect();
    if ids.is_empty() {
        return Err(FormulaError::NoAtoms);
    }
    let mut result = Composition::default();
    for id in ids {
        let atom = available.get(&id).ok_or(FormulaError::UnknownAtom(id))?;
        let element = atom.kind.element().ok_or(FormulaError::QueryAtom(id))?;
        if !atom.attachment_points.is_empty() {
            return Err(FormulaError::UnknownAttachment(id));
        }
        let hydrogen = match atom.hydrogens {
            Hydrogens::Specified(count) => u16::from(count),
            Hydrogens::Inferred if element.symbol() == "H" => 0,
            Hydrogens::Inferred => *inferred_hydrogens
                .get(&id)
                .ok_or(FormulaError::UnknownHydrogens(id))?,
            Hydrogens::Unknown => return Err(FormulaError::UnknownHydrogens(id)),
        };
        let count = result.species.entry((element, atom.isotope)).or_default();
        *count = count.checked_add(1).ok_or(FormulaError::CountOverflow)?;
        if hydrogen != 0 {
            let h = Element::from_symbol("H").expect("hydrogen is an element");
            let count = result.species.entry((h, None)).or_default();
            *count = count
                .checked_add(u32::from(hydrogen))
                .ok_or(FormulaError::CountOverflow)?;
        }
        result.charge = result
            .charge
            .checked_add(i32::from(atom.charge))
            .ok_or(FormulaError::CountOverflow)?;
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Masses {
    pub average_u: f64,
    pub exact_u: f64,
    /// Elements whose exact-profile value fell back to natural weight, not an isotope mass.
    pub natural_weight_fallbacks: Vec<Element>,
    /// Basis for each naturally distributed element's average; labeled isotopes use their mass.
    pub average_bases: Vec<(Element, AverageBasis)>,
    /// Source isotope rows with no abundance, omitted from a reported-fraction average.
    pub average_omitted_abundances: Vec<(Element, u16)>,
    pub exact_selections: Vec<(Element, u16, ExactBasis)>,
    pub exact_omitted_abundances: Vec<(Element, u16)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub mass_u: f64,
    /// Probability under the supplied abundance profile after the declared normalization.
    pub probability: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Distribution {
    pub peaks: Vec<Peak>,
    /// Raw natural-abundance sums before each element's fractions were normalized to one.
    pub abundance_sums: Vec<(Element, f64)>,
    /// Isotopes omitted because their abundance was absent in a conditional profile.
    pub omitted_abundances: Vec<(Element, u16)>,
}

fn validate(row: &ElementMass) -> Result<(), MassError> {
    let mut numbers = BTreeSet::new();
    if row
        .average
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
        || row.average.is_none() != (row.average_basis == AverageBasis::Unavailable)
        || row.isotopes.len() > MAX_PEAKS
    {
        return Err(MassError::InvalidData);
    }
    for isotope in &row.isotopes {
        if isotope.number == 0
            || !numbers.insert(isotope.number)
            || !isotope.mass.is_finite()
            || isotope.mass <= 0.0
        {
            return Err(MassError::InvalidData);
        }
        if let Some(value) = isotope.abundance
            && (!value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(MassError::InvalidData);
        }
    }
    Ok(())
}

impl Composition {
    /// Hill ordering: carbon, hydrogen, then other symbols if carbon is present; otherwise all
    /// symbols alphabetically. Explicit isotope mass numbers are printed in brackets.
    pub fn formula(&self) -> String {
        let contains_carbon = self
            .species
            .iter()
            .any(|((e, _), count)| e.symbol() == "C" && *count != 0);
        let mut rows: Vec<_> = self
            .species
            .iter()
            .filter(|(_, count)| **count != 0)
            .collect();
        rows.sort_by_key(|((element, isotope), _)| {
            let prefix = if contains_carbon {
                match element.symbol() {
                    "C" => 0,
                    "H" => 1,
                    _ => 2,
                }
            } else {
                0
            };
            (prefix, element.symbol(), *isotope)
        });
        let mut out = String::new();
        for ((element, isotope), count) in rows {
            if let Some(number) = isotope {
                out.push_str(&format!("[{number}{}]", element.symbol()));
            } else {
                out.push_str(element.symbol());
            }
            if *count != 1 {
                out.push_str(&count.to_string());
            }
        }
        if self.charge != 0 {
            let magnitude = self.charge.unsigned_abs();
            if magnitude != 1 {
                out.push('^');
                out.push_str(&magnitude.to_string());
            }
            out.push(if self.charge > 0 { '+' } else { '-' });
        }
        out
    }

    pub fn masses(&self, table: &MassTable) -> Result<Masses, MassError> {
        if !table.charge_correction.is_finite() {
            return Err(MassError::InvalidData);
        }
        let mut average = f64::from(self.charge) * table.charge_correction;
        let mut exact = average;
        let mut natural_weight_fallbacks = Vec::new();
        let mut average_bases = Vec::new();
        let mut average_omitted_abundances = Vec::new();
        let mut exact_selections = Vec::new();
        let mut exact_omitted_abundances = Vec::new();
        for (&(element, isotope), &count) in &self.species {
            if count == 0 {
                continue;
            }
            let row = table
                .elements
                .get(&element)
                .ok_or(MassError::MissingElement)?;
            validate(row)?;
            if isotope.is_none() {
                average_bases.push((element, row.average_basis));
                if row.average_basis == AverageBasis::ReportedIsotopeComposition {
                    average_omitted_abundances.extend(
                        row.isotopes
                            .iter()
                            .filter(|item| item.abundance.is_none())
                            .map(|item| (element, item.number)),
                    );
                }
            }
            // The pinned input profile falls back to the natural weight only when the
            // element has no isotope rows and no explicit isotope was requested.
            let selected_mass = if isotope.is_none() && row.isotopes.is_empty() {
                natural_weight_fallbacks.push(element);
                row.average.ok_or(MassError::MissingAverage)?
            } else {
                let chosen = isotope.unwrap_or(row.selected_isotope);
                let basis = if isotope.is_some() {
                    ExactBasis::ExplicitIsotope
                } else {
                    row.selection_basis
                };
                if basis == ExactBasis::Unavailable {
                    return Err(MassError::MissingIsotope);
                }
                exact_selections.push((element, chosen, basis));
                if basis == ExactBasis::MaximumReportedFraction {
                    exact_omitted_abundances.extend(
                        row.isotopes
                            .iter()
                            .filter(|item| item.abundance.is_none())
                            .map(|item| (element, item.number)),
                    );
                }
                row.isotopes
                    .iter()
                    .find(|item| item.number == chosen)
                    .ok_or(MassError::MissingIsotope)?
                    .mass
            };
            average += f64::from(count)
                * if isotope.is_some() {
                    selected_mass
                } else {
                    row.average.ok_or(MassError::MissingAverage)?
                };
            exact += f64::from(count) * selected_mass;
        }
        if !average.is_finite() || !exact.is_finite() {
            return Err(MassError::InvalidData);
        }
        Ok(Masses {
            average_u: average,
            exact_u: exact,
            natural_weight_fallbacks,
            average_bases,
            average_omitted_abundances,
            exact_selections,
            exact_omitted_abundances,
        })
    }

    /// Enumerate isotope compositions without merging equal-mass peaks or pruning. Complete
    /// input fractions within 0.1% of one are normalized; raw sums are returned explicitly.
    /// Missing abundance is not treated as zero, and underflow yields no partial distribution.
    pub fn distribution(&self, table: &MassTable) -> Result<Distribution, MassError> {
        if !table.charge_correction.is_finite() {
            return Err(MassError::InvalidData);
        }
        let mut peaks = vec![Peak {
            mass_u: f64::from(self.charge) * table.charge_correction,
            probability: 1.0,
        }];
        let mut abundance_sums = Vec::new();
        let mut omitted_abundances = Vec::new();
        let mut work = 0usize;
        for (&(element, selected), &count) in &self.species {
            if count == 0 {
                continue;
            }
            let row = table
                .elements
                .get(&element)
                .ok_or(MassError::MissingElement)?;
            validate(row)?;
            if let Some(number) = selected {
                let isotope = row
                    .isotopes
                    .iter()
                    .find(|item| item.number == number)
                    .ok_or(MassError::MissingIsotope)?;
                if !isotope.mass.is_finite() || isotope.mass <= 0.0 {
                    return Err(MassError::InvalidData);
                }
                for peak in &mut peaks {
                    peak.mass_u += f64::from(count) * isotope.mass;
                }
                continue;
            }
            let mut input = Vec::new();
            let mut sum = 0.0;
            for isotope in &row.isotopes {
                let Some(abundance) = isotope.abundance else {
                    if table.missing_abundance == MissingAbundancePolicy::Reject {
                        return Err(MassError::MissingAbundance);
                    }
                    omitted_abundances.push((element, isotope.number));
                    continue;
                };
                if !isotope.mass.is_finite()
                    || isotope.mass <= 0.0
                    || !abundance.is_finite()
                    || !(0.0..=1.0).contains(&abundance)
                {
                    return Err(MassError::InvalidData);
                }
                sum += abundance;
                if abundance > 0.0 {
                    input.push((*isotope, abundance));
                }
            }
            if input.is_empty() || !(0.999..=1.001).contains(&sum) {
                return Err(MassError::MissingAbundance);
            }
            abundance_sums.push((element, sum));
            for (_, abundance) in &mut input {
                *abundance /= sum;
            }
            let mut counts = BTreeMap::from([(vec![0_u32; input.len()], 1.0_f64)]);
            for _ in 0..count {
                let mut next = BTreeMap::new();
                for (key, probability) in counts {
                    for (i, (_, abundance)) in input.iter().enumerate() {
                        work = work
                            .checked_add(1)
                            .filter(|n| *n <= MAX_DISTRIBUTION_WORK)
                            .ok_or(MassError::ResourceLimit)?;
                        let mut updated = key.clone();
                        updated[i] += 1;
                        let contribution = probability * abundance;
                        if contribution == 0.0 {
                            return Err(MassError::ProbabilityUnderflow);
                        }
                        *next.entry(updated).or_insert(0.0) += contribution;
                        if next.len() > MAX_PEAKS {
                            return Err(MassError::ResourceLimit);
                        }
                    }
                }
                counts = next;
            }
            let mut next = Vec::new();
            for (counts, probability) in counts {
                let mass: f64 = counts
                    .iter()
                    .zip(&input)
                    .map(|(n, (iso, _))| f64::from(*n) * iso.mass)
                    .sum();
                for peak in &peaks {
                    if next.len() >= MAX_PEAKS {
                        return Err(MassError::ResourceLimit);
                    }
                    let probability = peak.probability * probability;
                    if probability == 0.0 {
                        return Err(MassError::ProbabilityUnderflow);
                    }
                    next.push(Peak {
                        mass_u: peak.mass_u + mass,
                        probability,
                    });
                }
            }
            peaks = next;
        }
        if peaks
            .iter()
            .any(|peak| !peak.mass_u.is_finite() || !peak.probability.is_finite())
        {
            return Err(MassError::InvalidData);
        }
        peaks.sort_by(|a, b| a.mass_u.total_cmp(&b.mass_u));
        Ok(Distribution {
            peaks,
            abundance_sums,
            omitted_abundances,
        })
    }
}
