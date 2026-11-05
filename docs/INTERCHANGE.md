# Interchange document and CDXML codec

The rich CDXML tree is available through operation contract 1 in the Rust
application, HTTP and CLI. The browser can preview a selected page and molecule
and create a new editable drawing after the user accepts its loss report. The HTTP/CLI
operation also exports a current drawing through the bounded P29e profile below;
P29f adds a web preview, loss-consent and download panel for that same profile. A bounded
CDX codec feeds the same tree and operations; see [CDX](#cdx-binary-codec).

`chemifree_domain::interchange::Document` is an ordered tree independent of the
editor's single-graph document. It can hold a document with several pages, nested
groups and fragments, nodes and bonds, positioned graphics, Unicode text and style
runs, font and color tables, IDs, and represented-property references. Node type is
an enumerated value: a node does not necessarily denote an element. The XML root
namespace is part of the tree, while namespace prefixes are serialization details.
The common model also has explicitly opaque XML and binary extension variants; an
unknown extension is not assumed to be free of object references.

Construction and atomic edits validate nonzero assigned IDs, duplicates, known
references, font and color references, bond endpoint type and same-fragment
membership, required graphic bounds and represented-property fields, property
types, finite coordinates, nesting and object limits. Unassigned IDs are represented
separately. When opaque content exists, edits cannot silently change an ID or the
opaque content; a caller may explicitly discard opaque content and report the loss.
A failed edit or failed discard leaves the document unchanged.

The CDXML reader accepts UTF-8 input up to 1 MiB and checks XML syntax, IDs and
known references. It does not fetch external DTDs, entities, or resources; DTD
internal subsets are rejected. The reader preserves an unchanged input byte for
byte, including uninterpreted extensions and outside-root material. A changed
document retains its UTF-8 BOM when one was present and cannot be rewritten while
opaque outside-root material remains without an explicit
discard decision. The application-facing codec re-parses any supplied original
and compares its decoded tree with the submitted tree; it does not trust a
client-supplied opaque baseline. It reports retained or dropped opaque material,
and does not mutate the submitted document on failure. Without an original, it
can validate only the submitted tree and cannot detect content a caller already
omitted or prove anything about that earlier source's unknown references.
Numeric unqualified IDs on opaque objects in the CDXML document's own namespace
directly under known objects are checked for duplicates and can be targets of
known references; their other properties remain uninterpreted. Identifiers on
foreign-namespace extensions, descendants of opaque objects and identifiers
outside the profile's unsigned-integer spelling remain opaque. If a known
reference could match an unqualified numeric `id` in such opaque data, the reader
returns `unsupported_opaque_reference` instead of guessing its meaning. If discarding an opaque object would
leave a known reference dangling, the discard fails atomically; the caller must
resolve the reference in a valid replacement before explicitly discarding it.
Unknown property values and unimplemented graphics, styles or references remain
opaque rather than being silently assigned another meaning.

An explicit CDXML [font table](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/FontTable.htm)
or [color table](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/ColorTable.htm)
must contain at least one corresponding entry; a missing table is distinct from
an empty one. With no color table, the documented default indexes 0 through 3
remain available. The CDX codec's own policy is below: a count-zero table is refused.

The known CDXML subset includes 2D positions and bounds, element numbers, isotope,
charge and radical values, node types, bond endpoints and order bitsets, basic
graphic type/bounds, represented-property targets, font and color table entries,
and styled text. Advanced chemistry, additional object properties and external
resources are not implemented by this CDXML core. A CDX unspecified bond
order is refused when it has no unambiguous CDXML representation; it is not
silently emitted as a single bond.

The editor projection selects one page and a top-level fragment, including one
inside groups. Element nodes and supported bond orders become an editable graph;
unsupported chemical node or bond types are refused. Every selected atom must have
an in-range position. Uninterpreted properties or unknown child objects on the
selected fragment, uninterpreted properties on selected nodes or bonds, and labels
or other child content on selected nodes or bonds are refused because their
chemical meaning is not established. The [public node specification](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/Node.htm)
permits a label to carry the chemical description. Rich read and unchanged export
remain available. In particular, dative bonds remain in the rich tree but
are refused by editor projection: the [public bond-order specification](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Bond_Order.htm)
describes the first endpoint as positive, while the editor's first endpoint is a
donor. The projection does not infer that these conventions coincide. A bond whose `Display` is outside the public list (or whose `Display2` is not a listed name) is refused by the projection as `unsupported_bond` rather than created as a plain bond, and the listed wedge, hashed wedge and wavy values are read as bond marks (see the contract's "CDXML and CDX stereo import"); drawing export now writes the supported marks through the bounded profile below. Default
implicit-hydrogen interpretation is reported as
uncertain because the editor's bounded inference need not agree with another
reader. Double bonds also report uncertainty about stereo another reader may
interpret from the drawing. Original IDs and source
paths are mapped to new editor IDs. Unselected pages and fragments, styled text,
graphics, groups, references, unknown properties outside selected chemistry, and
outside-root XML that the editor cannot store are reported as losses. A discarded
container also reports nested molecular fragments, opaque content and represented-property
references, including each known reference target and attribute. A preview does not create a document.
The create operation recomputes the source, selection, losses, settings and graph;
its digest also binds the preview to the running service. Changed input, settings
or a restart requires a new preview. Creation commits the complete document
before acknowledging and replays an already acknowledged request while the
service's request history is retained. It never overwrites an existing document.

The independently authored `crates/providers/tests/data/cdxml/multi-page.cdxml`
fixture is checked across graph, pages, graphics, text, styles and references. The
optional `scripts/verify-cdxml-xml.py` invokes the provider rewrite utility and
uses Python's standard-library XML reader to compare those axes against independent
expected values. It is a bounded check, not a claim of general CDXML compatibility.

Public format references: [CDXML introduction](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/IntroCDXML.htm),
[node types](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Node_Type.htm),
[graphics](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Graphic_Type.htm),
[bond orders](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/properties/Bond_Order.htm),
and [represented-property references](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/Represent.htm).

## Drawing export (P29e)

`document.export { format: "cdxml" | "cdx", document_id, accepted_losses? }`
exports a captured drawing and returns `content` plus its document/session `snapshot`.
It is separate from the unchanged rich-tree input form: do not combine `document_id`
with `document`, `original` or `discard_opaque`. The first placement becomes one page
and fragment; additional placements, native IDs, scene objects/references, selection
and settings have explicit losses. Supported element atoms, coordinates, charge,
isotope, radical states and single/double/triple/aromatic bonds are written. Query
atoms, dative/query/unknown bonds and unknown radicals are refused. Unwritten stated
hydrogens and attachment points require consent. P29f adds the web drawing-export
controls described below; it does not broaden the codec or projection profile.

Four properties have verified typed cross-format encodings: bond `BondDisplay`
(`Display`, tag `0601`, INT16 0–14), node `EnhancedStereoType` (`0446`, UINT8 0–4),
`EnhancedStereoGroupNum` (`0447`, UINT16 binary, int32 common model/CDXML), and
`ShowAtomEnhancedStereo` (`0445`, one-byte boolean, CDXML `yes`/`no`). Unknown names,
locations, namespaces, widths and values remain opaque under the existing preservation
policy. Known typed values can cross formats without discarding opaque extensions;
this does not make other extensions safe to rewrite.

Wedge/hash marks preserve the narrow end; wavy single and crossed double bonds use
`Wavy`. The exporter synthesizes no marks, CIP labels, `Geometry` or `BondOrdering`.
Stored configurations are checked against the output's marks and actual coordinates,
including fixed-point CDX quantization. A mismatch requires `stereo_configuration`
consent; stored double-bond statements require `double_bond_configuration` consent.
An absolute/and/or group is emitted only when every member's written drawing derives
the stored tetrahedral state; otherwise `stereo_group` reports the whole omitted group.
A member subset is never silently kept. Output numbers 1–127 are preserved; others
receive the lowest free positive number per kind after existing valid numbers are
reserved (`stereo_group_number`, informational normalization). A full number space
is refused as `stereo_group_limit`. Group IDs are informational `stereo_group_identity`
losses. Originally ungrouped stored centers require `stereo_ungrouped_centers` consent.

Required losses are shown in diagnostic `details.losses` with `loss_digest`, and no
content. Repeat with that digest as `accepted_losses`; changed losses or output bytes
invalidate it. Loss entries expose no `consent` field. CDX precision losses use the
same flow. Export changes neither source drawing nor history. Tests exercised for
this slice: `cdxml_stereo_export` (7), `cdxml_stereo_import` (34),
`stereo_carrier_typing` (5), and application unit tests (25). Full-format compatibility,
double-bond E/Z derivation and a general scene export remain outside this profile.

### Web drawing-export panel (P29f)

The **CDXML and CDX export** panel prepares either format and shows its losses. Required
losses need the explicit `Accept these losses and preview CDXML`/`CDX` action; no
download is offered before acceptance succeeds. An invalid or missing consent digest
cannot authorize a file. A changed digest asks for a new review. Informational loss
headings separately describe omitted group IDs, omitted groups and uncertainty about
ungrouped centers, without claiming all group content was preserved or every format
lacks group support.

Prepared results and offers belong to the full displayed snapshot, chosen format and
service generation. Edits, another drawing/format or a noticed restart invalidate them;
a delayed response cannot revive an old result. Download makes a fresh export request
and compares its snapshot, content and losses with the prepared result. CDXML downloads
as UTF-8 text; CDX is decoded from canonical transport base64 into binary bytes, never
saved as a base64 text file. The files are `chemifree.cdxml`/`chemifree.cdx`. The panel
synthesizes no marks or labels and retains all P29e semantic limits and loss rules.
P29 remains `in_progress` pending P29g. Client and browser checks are defined, but their
final verification is not asserted by this documentation update.

## CDX binary codec

`crates/providers/src/cdx/` reads and writes the public
[CDX file format](https://iupac.github.io/IUPAC-FAIRSpec/cdx_sdk/IntroCDX.htm) into the same
`Document` tree as CDXML. It was written from the public documentation only. Opaque
policy is shared with the CDXML write path (`opaque_policy.rs`): an unchanged original is
returned byte for byte (also with `discard_opaque` when the original holds no opaque content, so
there is nothing to discard), an edited document with opaque content is refused
(`unsafe_unknown_rewrite`) unless `discard_opaque`, and a discard is reported as a
`dropped` `opaque_extensions` loss. The recorded CDXML outputs in
`crates/providers/tests/data/cdxml-golden/` pin the P22 results taken before the CDX code existed.

**Framing.** A 28-byte header (`VjCD0100`, then 20 reserved bytes that are not interpreted; a rewrite
keeps those of the `original` and, with none, writes the values the specification shows), a Document object (tag `0x8000`, ID, properties,
child objects, `0000`) and the two-byte end-of-file marker. This codec requires the marker, so every
proper prefix of a file is refused as `truncated` (or `invalid_*`), and bytes after it are
`trailing_data`. That strictness is unverified against the specification: the public
documentation says "the end of the CDX file is marked with two bytes of 0 (00 00)" and that every
object ends with `0000`, but it does not say whether a reader must refuse a file that lacks the
marker. A file without it (Document ends, then nothing) is refused as `truncated` at the marker
until that is decided; the writer always emits it. Property lengths use the 16-bit field, or `0xFFFF` plus a 32-bit length; the
writer uses the extended form exactly from 65535 bytes, the reader also accepts it for shorter
values. Lengths are checked against the remaining bytes before use. A Document ID of 0 means "no
ID assigned" and reads as no ID (the model cannot tell them apart; CDXML `id="0"` does the
same); the writer always emits the header and a Document object, with ID 0 when the root has
none. Object IDs must be unique across the document (the format asks only for uniqueness within
a container, a looser rule this codec does not accept).

**Limits**, checked before allocation and counted as the common model counts them: 1 MiB of
bytes, depth 128 (root is depth 0), 100,000 objects including the objects the model creates from
properties (font table + fonts, color table + colors, style runs, Represents). The refusal is
`resource_limit` with the offset of the first item that exceeds the limit.

**Interpreted subset**, mirroring the CDXML reader: Document, Page, Group, Fragment, Node, Bond,
Text, Graphic; bounding box, 2D position, node type, element, isotope, charge, radical, bond
order, bond endpoints, graphic type, font table (`0x0100`), color table (`0x0300`), text string
(`0x0700`), Represents (`0x000E`) and the four typed stereo carriers described above. Everything else, a known tag on an object the CDXML
reader would not interpret it on, and every descendant of an unknown object, stays opaque
binary. Coordinates are 1/65536 point (y before x; rectangles top, left, bottom, right).
Integers are read by the width the file gives them (1, 2 or 4 bytes). A known value outside
its documented range (node type 0..13, element 1..118, isotope 0..32767, radical 0..3, graphic
type 0..7, bond order 1..65535) is refused with `out_of_range` and its offset; this differs from
CDXML, where an unknown enumeration *name* stays opaque, because a number outside the table
has no name to keep. A 4-byte charge is `saved >> 24` and refused when its low 24 bits are not
zero. Bond order `0xFFFF` (unspecified) is kept and cannot be written as CDXML. A Represents
property naming a tag without a verified CDXML attribute name stays opaque; a second such
property on one graphic is refused.

**Text.** The public documentation names no character sets beyond the example pairing code page
1252 with CDXML `iso-8859-1`, so that is the only pairing accepted; a font table entry with
another character set is refused with `unsupported_charset` at the entry, even if no text uses
the font. A string without style runs is ISO Latin 1. Bytes 0x80 to 0x9F are refused in styled
and unstyled text (`unsupported_text_encoding`), control characters XML 1.0 cannot carry are
`invalid_text` (the character rule is the CDXML one), and text of a Macintosh-platform font
(table platform 0) is accepted only as ASCII. Style runs must start at 0 in order within the
text and use a known font and color. A run with no characters has no children, exactly as in
the CDXML reader, so CDX to CDXML to CDX and back give identical trees. An empty font table
or color table is refused (the specification requires at least one font; the model requires one
color).

**Writer.** Font and color tables become document properties ahead of the objects (a
CDXML table after a page is moved and `child_order` is reported). Font tables are written for
platform 1, except that a table equal to the same table of the re-parsed `original` keeps the
platform it was read with (0 or 1) and reports nothing; `font_platform` is reported for every
other written table (the platform is not known, so Windows is written) and never for a plain read.
Text in a style run whose font belongs to a kept Macintosh table must be ASCII, as the reader
requires (`unsupported_text`). Numbers
that are not exact steps are rounded to the nearest one and reported as `precision`:
coordinates to 1/65536 point (error at most 1/131072), color components to 1/65535, font
sizes to 1/20 point. Values outside the range (coordinates beyond +-32768 pt, sizes above
3276.75, colors) are refused (`out_of_range`). Text must be Latin 1 without U+0080 to U+009F
(`unsupported_text`); fonts need a supported charset (`unsupported_charset`,
`missing_property`). A single style run without attributes is an unstyled string; otherwise
each run needs a font and a size (`unsupported_style_run`); a missing face is written as
plain (0) and a missing color as color index 0, and both are reported as `style_defaults`. The
public documentation fixes color indexes 0 and 1 as black and white in every document, whatever
its color table holds, so index 0 (black) is the one default that always exists; the
document foreground (index 3) is the table's second entry and does not exist in every table. Whitespace-only text between runs is ignored; XML
comments, instructions, stray text outside runs, the root namespace and Represents objects
naming an unverified attribute have no CDX encoding and are reported as `dropped`
(`xml_content`, `xml_namespace`, `represents`); all but the namespace, which carries no
meaning, need the caller's consent (`accepted_losses`, bound to what was shown).
A Represents object with its own ID is refused. Object IDs on style runs, font and color tables and their entries cannot be written (CDX numbers only real objects); they are dropped with a reported `object_ids` loss, and a reference that names one is refused (`unsupported_object_id`). The properties of an unknown object, and of every object inside it, are kept as found, in
order, repeats included (an unchanged original returns them byte for byte). A font-table or Represents property repeated where it has no meaning (any
other repeat in a known object), a Text without its string, and a Represents at the depth limit are refused with `duplicate_property`, `missing_property` and `resource_limit` at the property or object offset. What the writer emits is re-read to the same
tree by the reader.

**Model errors.** CDX reports a failure found only by the common model with the domain's specific code and message; CDXML keeps its P22 code and wording. The difference and the reason are stated once, in [CONTRACT.md](CONTRACT.md#cdx-over-the-same-operations).

**Not implemented:** CDX objects beyond the subset (reactions, brackets, curves, embedded
objects, tables...), other character sets, Macintosh text beyond ASCII, 3D positions, and any
chemistry interpretation. The fixture `multi-page.cdxml` uses charset `utf-8` and so cannot be
written as CDX.

Checks: `crates/providers/tests/cdx_reader.rs`, `cdx_writer.rs`, `cdx_cross_format.rs` (seeded
CDX/CDXML sweeps over 400 documents plus truncation and mutation sweeps),
`crates/application/tests/cdx_operations.rs`, `apps/api/tests/cdx_transport.rs`,
`apps/cli/tests/cdx_transport.rs`, conformance case `cdx-binary-interchange`, and
`scripts/verify-cdx-semantics.py`, which parses CDX bytes with its own code and compares graph,
text runs, styles and colors with the CDXML reading.
