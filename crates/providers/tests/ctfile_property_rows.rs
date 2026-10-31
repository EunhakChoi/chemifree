use chemifree_application::ctfile::{CtCodec, CtFormat, CtRadical};
use chemifree_providers::V2000Codec;

#[test]
fn independent_v2000_property_rows_apply_across_the_eight_entry_boundary() {
    // CTfile property records carry at most eight atom/value pairs per row. The fixture
    // is independently authored, rather than the output of this codec's writer.
    let mut source = String::from("Rows\n\n\n  9  0  0  0  0  0  0  0  0  0999 V2000\n");
    for _ in 0..9 {
        source.push_str("    0.0000    0.0000    0.0000 C   0  3  0  0  0  0  0  0  0  0  0  0\n");
    }
    source.push_str("M  CHG  8   1   1   2  -1   3   1   4  -1   5   1   6  -1   7   1   8  -1\n");
    source.push_str("M  CHG  1   9  -2\n");
    source.push_str("M  RAD  8   1   2   2   0   3   2   4   0   5   2   6   0   7   2   8   0\n");
    source.push_str("M  RAD  1   9   3\n");
    source.push_str("M  ISO  8   1  13   2  14   3  13   4  14   5  13   6  14   7  13   8  14\n");
    source.push_str("M  ISO  1   9  13\nM  END\n");
    let atoms = V2000Codec
        .read(CtFormat::Mol, source.as_bytes())
        .unwrap()
        .value
        .remove(0)
        .atoms;
    assert_eq!(
        atoms.iter().map(|a| a.charge).collect::<Vec<_>>(),
        [1, -1, 1, -1, 1, -1, 1, -1, -2]
    );
    assert_eq!(
        atoms.iter().map(|a| a.radical).collect::<Vec<_>>(),
        [
            CtRadical::Doublet,
            CtRadical::None,
            CtRadical::Doublet,
            CtRadical::None,
            CtRadical::Doublet,
            CtRadical::None,
            CtRadical::Doublet,
            CtRadical::None,
            CtRadical::Triplet
        ]
    );
    assert_eq!(
        atoms.iter().map(|a| a.isotope).collect::<Vec<_>>(),
        [
            Some(13),
            Some(14),
            Some(13),
            Some(14),
            Some(13),
            Some(14),
            Some(13),
            Some(14),
            Some(13)
        ]
    );
}
