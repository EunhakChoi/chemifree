//! Interrupting an isotope distribution either stops it with `Interrupted` or changes nothing
//! about its result: a stop is never a partial distribution or a refusal, and an uninterrupted
//! result is compared bit for bit.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;

use chemifree_domain::budget::{Interrupt, Interrupted, NEVER};
use chemifree_domain::element::Element;
use chemifree_domain::mass::{
    AverageBasis, Composition, Distribution, ElementMass, ExactBasis, Isotope, MassError,
    MassTable, MissingAbundancePolicy,
};

fn carbon() -> Element {
    Element::from_symbol("C").unwrap()
}

fn isotope(number: u16, mass: f64, abundance: Option<f64>) -> Isotope {
    Isotope {
        number,
        mass,
        abundance,
    }
}

/// A table whose only element is carbon, with the given isotopes and missing-abundance policy.
fn carbon_table(isotopes: Vec<Isotope>, missing_abundance: MissingAbundancePolicy) -> MassTable {
    MassTable {
        elements: BTreeMap::from([(
            carbon(),
            ElementMass {
                average: Some(12.011),
                average_basis: AverageBasis::ProfileNaturalWeight,
                selected_isotope: 12,
                selection_basis: ExactBasis::PinnedProfileSelection,
                isotopes,
            },
        )]),
        charge_correction: -0.0005485799090441,
        missing_abundance,
    }
}

/// Sixty carbons. Carbon-12 and carbon-13 have reported abundances; carbon-14 has none, so the
/// conditioned distribution omits it. Each of the 60 steps extends every partial composition by
/// two isotopes, and the partial compositions processed in all are 1 + 2 + ... + 60 = 1,830, so
/// the enumeration takes 3,660 units of work.
fn sixty_carbons() -> (Composition, MassTable) {
    let table = carbon_table(
        vec![
            isotope(12, 12.0, Some(0.9893)),
            isotope(13, 13.00335483507, Some(0.0107)),
            isotope(14, 14.0032419884, None),
        ],
        MissingAbundancePolicy::ConditionOnReported,
    );
    let composition = Composition {
        species: BTreeMap::from([((carbon(), None), 60)]),
        charge: 0,
    };
    (composition, table)
}

/// A float that equals another only when both have the same bits, so `0.0` and `-0.0` differ.
/// Its Debug output shows the value next to its bits.
#[derive(Clone, Copy)]
struct Exact(f64);

impl PartialEq for Exact {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl fmt::Debug for Exact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} ({:#018x})", self.0, self.0.to_bits())
    }
}

/// A distribution whose every float is compared exactly.
#[derive(Debug, PartialEq)]
struct Bits {
    peaks: Vec<(Exact, Exact)>,
    abundance_sums: Vec<(Element, Exact)>,
    omitted_abundances: Vec<(Element, u16)>,
}

/// The whole outcome of a call, with any distribution read as its bits.
type Outcome = Result<Result<Bits, MassError>, Interrupted>;

fn bits(outcome: Result<Result<Distribution, MassError>, Interrupted>) -> Outcome {
    outcome.map(|inner| {
        inner.map(|distribution| Bits {
            peaks: distribution
                .peaks
                .iter()
                .map(|peak| (Exact(peak.mass_u), Exact(peak.probability)))
                .collect(),
            abundance_sums: distribution
                .abundance_sums
                .iter()
                .map(|&(element, sum)| (element, Exact(sum)))
                .collect(),
            omitted_abundances: distribution.omitted_abundances,
        })
    })
}

/// The outcome of the distribution of `composition` when `interrupt` is asked.
fn run(composition: &Composition, table: &MassTable, interrupt: &dyn Interrupt) -> Outcome {
    bits(composition.distribution_with(table, interrupt))
}

/// The probability of `k` carbon-13 atoms among `n` carbons, when each is carbon-13 with
/// probability `p`. This closed form shares no code with the enumeration it checks.
fn binomial_probability(n: u32, k: u32, p: f64) -> f64 {
    let choose = (1..=k).fold(1.0, |chosen, i| {
        chosen * f64::from(n - k + i) / f64::from(i)
    });
    choose * p.powf(f64::from(k)) * (1.0 - p).powf(f64::from(n - k))
}

/// Counts its questions, numbered from 1, and stops at the `stop_at`-th one when there is one.
struct StopAt {
    stop_at: Option<usize>,
    reason: Interrupted,
    calls: Cell<usize>,
}

impl StopAt {
    fn counting() -> Self {
        Self::new(None, Interrupted::Cancelled)
    }

    fn new(stop_at: Option<usize>, reason: Interrupted) -> Self {
        Self {
            stop_at,
            reason,
            calls: Cell::new(0),
        }
    }
}

impl Interrupt for StopAt {
    fn check(&self) -> Result<(), Interrupted> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if self.stop_at == Some(call) {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

#[test]
fn an_interrupt_that_never_stops_gives_the_unbudgeted_distribution() {
    let (composition, table) = sixty_carbons();
    let expected = composition.distribution(&table);
    let distribution = expected
        .as_ref()
        .expect("sixty carbons is a supported composition");
    // The comparison covers the peaks, the abundance sums and the omissions, so the corpus must
    // have each of them: one peak per count of carbon-13, from 0 to 60, and carbon-14 omitted.
    assert_eq!(
        distribution.peaks.len(),
        61,
        "sixty carbons in two isotopes have 61 peaks"
    );
    assert_eq!(
        distribution.omitted_abundances,
        [(carbon(), 14)],
        "carbon-14 has no reported abundance, so it must be omitted"
    );
    assert_eq!(
        distribution.abundance_sums.len(),
        1,
        "carbon is the only element"
    );
    let (element, sum) = distribution.abundance_sums[0];
    assert_eq!(element, carbon());
    assert!(
        (sum - 1.0).abs() < 1e-12,
        "the reported abundances sum to one, got {sum}"
    );
    // Independent of the enumeration: the peaks are the binomial distribution of the number k of
    // carbon-13 atoms, in order of mass, where each carbon-13 is taken with probability p.
    let p = 0.0107 / (0.9893 + 0.0107);
    for (k, peak) in (0..=60).zip(&distribution.peaks) {
        let mass = f64::from(60 - k) * 12.0 + f64::from(k) * 13.00335483507;
        assert!(
            (peak.mass_u - mass).abs() < 1e-9,
            "peak {k} has mass {} where {mass} is expected",
            peak.mass_u
        );
        let probability = binomial_probability(60, k, p);
        assert!(
            (peak.probability - probability).abs() <= 1e-12 * probability,
            "peak {k} has probability {} where {probability} is expected",
            peak.probability
        );
    }
    let uninterrupted = bits(Ok(expected));
    let counter = StopAt::counting();
    assert_eq!(
        run(&composition, &table, &counter),
        uninterrupted,
        "an interrupt that never stops must give the unbudgeted distribution, bit for bit"
    );
    assert_eq!(
        run(&composition, &table, &NEVER),
        uninterrupted,
        "NEVER must give the unbudgeted distribution, bit for bit"
    );
}

#[test]
fn the_enumeration_asks_once_for_every_1024_units_of_work() {
    let (composition, table) = sixty_carbons();
    let counter = StopAt::counting();
    composition
        .distribution_with(&table, &counter)
        .expect("the counting interrupt never stops")
        .expect("sixty carbons is a supported composition");
    assert_eq!(
        counter.calls.get(),
        3,
        "3,660 units of work ask once per 1,024 units, at units 1,024, 2,048 and 3,072"
    );
}

#[test]
fn a_stop_at_any_question_stops_the_enumeration_and_a_later_stop_changes_nothing() {
    let (composition, table) = sixty_carbons();
    let uninterrupted = bits(Ok(composition.distribution(&table)));
    // The first question is the earliest point at which a stop can take effect.
    assert_eq!(
        run(
            &composition,
            &table,
            &StopAt::new(Some(1), Interrupted::TimeLimit)
        ),
        Err(Interrupted::TimeLimit),
        "a time limit at the first question must stop the enumeration, with no distribution"
    );
    let counter = StopAt::counting();
    assert_eq!(
        run(&composition, &table, &counter),
        uninterrupted,
        "the counting run gives the unbudgeted distribution"
    );
    let total = counter.calls.get();
    for stop_at in 1..=total {
        for reason in [Interrupted::TimeLimit, Interrupted::Cancelled] {
            assert_eq!(
                run(&composition, &table, &StopAt::new(Some(stop_at), reason)),
                Err(reason),
                "{reason:?} at question {stop_at} of {total} must stop the enumeration, with no \
                 partial distribution"
            );
        }
    }
    assert_eq!(
        run(
            &composition,
            &table,
            &StopAt::new(Some(total + 1), Interrupted::TimeLimit)
        ),
        uninterrupted,
        "a stop after the last question changes nothing"
    );
}

#[test]
fn a_composition_refused_for_its_resource_limit_stays_a_refusal() {
    // Two equally abundant isotopes of 5,000 carbons need more than MAX_DISTRIBUTION_WORK units.
    // The work limit is refused at the 1,000,001st unit, before that unit asks a question. The
    // questions before it are at each multiple of 1,024 up to 1,000,000: 976 of them.
    let table = carbon_table(
        vec![
            isotope(12, 12.0, Some(0.5)),
            isotope(13, 13.00335483507, Some(0.5)),
        ],
        MissingAbundancePolicy::Reject,
    );
    let huge = Composition {
        species: BTreeMap::from([((carbon(), None), 5_000)]),
        charge: 0,
    };
    let counter = StopAt::counting();
    assert_eq!(
        run(&huge, &table, &counter),
        Ok(Err(MassError::ResourceLimit)),
        "a never-stopping interrupt must keep the work limit a refusal, not an interruption"
    );
    assert_eq!(
        counter.calls.get(),
        976,
        "the work limit refuses at unit 1,000,001, after 976 questions at units up to 1,000,000"
    );
    assert_eq!(
        run(&huge, &table, &StopAt::new(Some(1), Interrupted::TimeLimit)),
        Err(Interrupted::TimeLimit),
        "a time limit at the first question stops the enumeration before its refusal"
    );
    assert_eq!(
        huge.distribution(&table),
        Err(MassError::ResourceLimit),
        "the unbudgeted enumeration refuses the same work limit"
    );
}
