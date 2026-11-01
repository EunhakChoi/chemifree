//! The radical state of a bracket atom, read from the shared valence model.
//!
//! SMILES has no radical field: `[CH3]` is a methyl radical because three hydrogens and no
//! bond leave carbon one valence short. The reader derives the state with
//! `chemistry::assess` and the writer uses the same function to check that the text it writes
//! reads back as the recorded state.

use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId};
use crate::states::{Hydrogens, Radical};

/// How a bracket atom's radical state was obtained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadicalNote {
    /// The hydrogens and bonds fill a valence: no radical, nothing inferred.
    Stated,
    /// One valence is open and the model reads it as a doublet.
    Inferred,
    /// The model cannot decide: the atom or charge is outside its table, or two open
    /// valences could be a singlet or a triplet. The drawing records an unknown radical.
    Undetermined,
}

/// The radical state of `atom` (whose hydrogens are the bracket's count) given its incident
/// bond orders, and how it was decided. A state that fits no valence (such as a carbon with
/// one hydrogen and no bond) is recorded as no radical; the valence inspection reports it.
pub(crate) fn infer(atom: &Atom, hydrogens: u8, orders: &[Option<u16>]) -> (Radical, RadicalNote) {
    let id = AtomId::from_serial(1).expect("serial 1 is a valid atom ID");
    let assess = |radical: Radical| {
        let mut probe = atom.clone();
        probe.hydrogens = Hydrogens::Specified(hydrogens);
        probe.radical = radical;
        chemistry::assess(id, &probe, orders)
    };
    let plain = assess(Radical::None);
    match plain.finding {
        Finding::Valid => (Radical::None, RadicalNote::Stated),
        Finding::Unsupported => (Radical::Unknown, RadicalNote::Undetermined),
        Finding::Invalid => {
            if assess(Radical::Doublet).finding == Finding::Valid {
                (Radical::Doublet, RadicalNote::Inferred)
            } else if assess(Radical::Singlet).finding == Finding::Valid
                && assess(Radical::Triplet).finding == Finding::Valid
            {
                (Radical::Unknown, RadicalNote::Undetermined)
            } else {
                (Radical::None, RadicalNote::Stated)
            }
        }
    }
}
