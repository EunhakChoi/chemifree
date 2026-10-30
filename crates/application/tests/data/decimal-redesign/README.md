# Independent decimal-order examples

These invented tokens do not contain chemical data. `generate.mjs` constructs
300 comparisons from seed `0x1743a5c9`, evaluates each as an integer numerator
and a power-of-ten denominator using JavaScript `BigInt`, and writes the fixture.
The implementation under test uses a different decimal ordering representation.
The files are released under the repository's MIT license.
