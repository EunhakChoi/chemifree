# Interchange document and CDXML codec

This document describes the in-process interchange core. The HTTP, CLI and browser
operation paths are still being integrated; this core alone does not claim P22 or
P23 is complete.

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
document cannot be rewritten while such material remains without an explicit
discard decision. The application-facing codec re-parses any supplied original
and compares its decoded tree with the submitted tree; it does not trust a
client-supplied opaque baseline. It reports retained or dropped opaque material,
and does not mutate the submitted document on failure. Unknown property values and
unimplemented graphics, styles or references remain opaque rather than being
silently assigned another meaning.

The known CDXML subset includes 2D positions and bounds, element numbers, isotope,
charge and radical values, node types, bond endpoints and order bitsets, basic
graphic type/bounds, represented-property targets, font and color table entries,
and styled text. Advanced chemistry, additional object properties, external
resources and CDX are not implemented by this CDXML core. A CDX unspecified bond
order is refused when it has no unambiguous CDXML representation; it is not
silently emitted as a single bond. Importing this interchange tree into the editor
will require a separate projection with a loss report for anything it cannot keep.

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
