"""Compare CDX and CDXML readings with a CDX parser that shares no Rust code.

Run from the repository root with `python -I` through the shared build wrapper,
because it invokes the provider's `cdx-convert` example. It assembles CDX bytes
itself, parses them itself, and compares the graph, text runs, styles and colors
with Python's standard-library XML reading of the codec's CDXML output; it also
converts a CDXML fixture to CDX and parses that. It checks one bounded fixture
and does not claim general CDX compatibility.
"""

# ruff: noqa: T201 - This standalone verification command prints its result.

from __future__ import annotations

import pathlib
import shutil
import struct
import subprocess
from typing import Any
import xml.etree.ElementTree

ROOT = pathlib.Path(__file__).resolve().parent.parent

_ORDER_NAMES = (
    (0x0001, "1"), (0x0002, "2"), (0x0004, "3"), (0x0008, "4"),
    (0x0010, "5"), (0x0020, "6"), (0x0040, "0.5"), (0x0080, "1.5"),
    (0x0100, "2.5"), (0x0200, "3.5"), (0x0400, "4.5"), (0x0800, "5.5"),
    (0x1000, "dative"), (0x2000, "ionic"), (0x4000, "hydrogen"),
    (0x8000, "threecenter"),
)  # fmt: skip
_NODE_TYPES = {0: "Unspecified", 1: "Element", 4: "Nickname"}
_GRAPHIC_TYPES = {1: "Line", 3: "Rectangle"}
_ATTRIBUTES = {0x0421: "Charge", 0x0420: "Isotope", 0x0402: "Element"}


def _prop(tag: int, data: bytes) -> bytes:
    if len(data) >= 0xFFFF:
        return struct.pack("<HHI", tag, 0xFFFF, len(data)) + data
    return struct.pack("<HH", tag, len(data)) + data


def _obj(tag: int, object_id: int, *parts: bytes) -> bytes:
    return struct.pack("<HI", tag, object_id) + b"".join(parts) + b"\0\0"


def _fixture() -> bytes:
    """Assembles a CDX file from the public byte layout."""
    header = b"VjCD0100" + bytes([4, 3, 2, 1]) + bytes(16)

    def point(x: float, y: float) -> bytes:
        return struct.pack("<ii", round(y * 65536), round(x * 65536))

    node_one = _obj(
        0x8004,
        3,
        _prop(0x0200, point(10, 20)),
        _prop(0x0402, struct.pack("<h", 6)),
    )
    node_two = _obj(
        0x8004,
        4,
        _prop(0x0200, point(25.5, -20)),
        _prop(0x0402, struct.pack("<h", 8)),
        _prop(0x0421, struct.pack("<b", -1)),
        _prop(0x0420, struct.pack("<h", 18)),
    )
    bond = _obj(
        0x8005,
        5,
        _prop(0x0604, struct.pack("<I", 3)),
        _prop(0x0605, struct.pack("<I", 4)),
        _prop(0x0600, struct.pack("<H", 2)),
    )
    runs = [(3, 1, 200, 2, b"Caf\xe9 "), (4, 0, 240, 3, b"H2O")]
    text_value = struct.pack("<H", len(runs))
    start = 0
    for font, face, size, color, text in runs:
        text_value += struct.pack("<HHHHH", start, font, face, size, color)
        start += len(text)
    text_value += b"".join(run[4] for run in runs)
    label = _obj(
        0x8006, 6, _prop(0x0200, point(40, 50)), _prop(0x0700, text_value)
    )
    graphic = _obj(
        0x8007,
        7,
        _prop(
            0x0204,
            struct.pack("<iiii", 2 * 65536, 65536, 40 * 65536, 30 * 65536),
        ),
        _prop(0x0A00, struct.pack("<h", 3)),
        _prop(0x000E, struct.pack("<IH", 4, 0x0421)),
    )
    fonts = struct.pack("<HH", 1, 2)
    for font_id, name in ((3, b"Arial"), (4, b"Times New Roman")):
        fonts += struct.pack("<HHH", font_id, 1252, len(name)) + name
    colors = struct.pack("<H", 3) + struct.pack(
        "<9H", 65535, 65535, 65535, 0, 0, 0, 65535, 0, 0
    )
    page = _obj(
        0x8001, 1, _obj(0x8003, 2, node_one, node_two, bond), label, graphic
    )
    document = _obj(
        0x8000, 0, _prop(0x0100, fonts), _prop(0x0300, colors), page
    )
    return header + document + b"\0\0"


class _Reader:
    """Parses the CDX subset used by the fixture into comparable axes."""

    def __init__(self, data: bytes) -> None:
        if data[:8] != b"VjCD0100":
            raise ValueError("not a CDX file")
        self._data = data
        self._position = 28
        self.fonts: dict[int, tuple[str, int]] = {}
        self.colors: list[tuple[float, ...]] = []

    def _tag(self) -> int:
        (value,) = struct.unpack_from("<H", self._data, self._position)
        self._position += 2
        return int(value)

    def document(self) -> dict[str, Any]:
        """Returns fonts, colors and pages of the file's Document object."""
        if self._tag() != 0x8000:
            raise ValueError("first object is not a Document")
        self._position += 4
        pages = []
        while (tag := self._tag()) != 0:
            if tag & 0x8000:
                self._position += 4
                pages.append(self._page())
            else:
                self._document_property(tag, self._value())
        if self._tag() != 0 or self._position != len(self._data):
            raise ValueError("bad end of file")
        return {
            "fonts": sorted((key, *value) for key, value in self.fonts.items()),
            "colors": self.colors,
            "pages": pages,
        }

    def _value(self) -> bytes:
        (length,) = struct.unpack_from("<H", self._data, self._position)
        self._position += 2
        if length == 0xFFFF:
            (length,) = struct.unpack_from("<I", self._data, self._position)
            self._position += 4
        value = self._data[self._position : self._position + length]
        self._position += length
        return value

    def _document_property(self, tag: int, value: bytes) -> None:
        if tag == 0x0100:
            (_, count) = struct.unpack_from("<HH", value)
            position = 4
            for _ in range(count):
                font_id, charset, size = struct.unpack_from(
                    "<HHH", value, position
                )
                name = value[position + 6 : position + 6 + size].decode(
                    "latin-1"
                )
                if charset != 1252:
                    raise ValueError("unexpected charset")
                self.fonts[font_id] = (name, charset)
                position += 6 + size
        elif tag == 0x0300:
            (count,) = struct.unpack_from("<H", value)
            for index in range(count):
                red, green, blue = struct.unpack_from(
                    "<HHH", value, 2 + 6 * index
                )
                self.colors.append((red / 65535, green / 65535, blue / 65535))

    def _page(self) -> dict[str, Any]:
        page: dict[str, Any] = {"fragments": [], "texts": [], "graphics": []}
        while (tag := self._tag()) != 0:
            object_id = struct.unpack_from("<I", self._data, self._position)[0]
            self._position += 4
            if tag == 0x8003:
                page["fragments"].append(self._fragment(object_id))
            elif tag == 0x8006:
                page["texts"].append(self._text(object_id))
            elif tag == 0x8007:
                page["graphics"].append(self._graphic(object_id))
            else:
                raise ValueError(f"unexpected page child {tag:#x}")
        return page

    def _fragment(self, object_id: int) -> dict[str, Any]:
        fragment: dict[str, Any] = {"id": object_id, "nodes": [], "bonds": []}
        while (tag := self._tag()) != 0:
            child_id = struct.unpack_from("<I", self._data, self._position)[0]
            self._position += 4
            properties = self._properties()
            if tag == 0x8004:
                fragment["nodes"].append(_node(child_id, properties))
            elif tag == 0x8005:
                bits = struct.unpack("<H", properties[0x0600])[0]
                names = " ".join(
                    name for bit, name in _ORDER_NAMES if bits & bit
                )
                fragment["bonds"].append(
                    (
                        child_id,
                        struct.unpack("<I", properties[0x0604])[0],
                        struct.unpack("<I", properties[0x0605])[0],
                        names,
                    )
                )
        return fragment

    def _properties(self) -> dict[int, bytes]:
        """Reads an object's properties up to its end marker."""
        found: dict[int, bytes] = {}
        while (tag := self._tag()) != 0:
            found[tag] = self._value()
        return found

    def _text(self, object_id: int) -> dict[str, Any]:
        properties = self._properties()
        value = properties[0x0700]
        (count,) = struct.unpack_from("<H", value)
        header_end = 2 + 10 * count
        runs = []
        for index in range(count):
            start, font, face, size, color = struct.unpack_from(
                "<HHHHH", value, 2 + 10 * index
            )
            following = (
                struct.unpack_from("<H", value, 2 + 10 * (index + 1))[0]
                if index + 1 < count
                else len(value) - header_end
            )
            text = value[header_end + start : header_end + following]
            runs.append((font, size / 20, face, color, text.decode("latin-1")))
        y, x = struct.unpack("<ii", properties[0x0200])
        return {"id": object_id, "p": (x / 65536, y / 65536), "runs": runs}

    def _graphic(self, object_id: int) -> dict[str, Any]:
        properties = self._properties_with_represents()
        top, left, bottom, right = (
            value / 65536
            for value in struct.unpack("<iiii", properties[0x0204])
        )
        target, attribute = struct.unpack("<IH", properties[0x000E])
        return {
            "id": object_id,
            "box": (left, top, right, bottom),
            "type": _GRAPHIC_TYPES[struct.unpack("<h", properties[0x0A00])[0]],
            "represents": (target, _ATTRIBUTES[attribute]),
        }

    _properties_with_represents = _properties


def _node(object_id: int, properties: dict[int, bytes]) -> tuple[Any, ...]:
    y, x = struct.unpack("<ii", properties[0x0200])
    return (
        object_id,
        struct.unpack("<h", properties[0x0402])[0],
        struct.unpack("<b", properties[0x0421])[0]
        if 0x0421 in properties
        else 0,
        struct.unpack("<h", properties[0x0420])[0]
        if 0x0420 in properties
        else 0,
        (x / 65536, y / 65536),
    )


def _xml_axes(source: str) -> dict[str, Any]:
    """Reads CDXML produced by the codec into the same axes."""
    root = xml.etree.ElementTree.fromstring(source)  # noqa: S314 - local output
    pages = []
    for page in root.findall("page"):
        fragments = []
        for fragment in page.findall("fragment"):
            nodes = [
                (
                    int(node.attrib["id"]),
                    int(node.get("Element", "6")),
                    int(node.get("Charge", "0")),
                    int(node.get("Isotope", "0")),
                    tuple(float(part) for part in node.attrib["p"].split()),
                )
                for node in fragment.findall("n")
            ]
            bonds = [
                (
                    int(bond.attrib["id"]),
                    int(bond.attrib["B"]),
                    int(bond.attrib["E"]),
                    bond.get("Order", "1"),
                )
                for bond in fragment.findall("b")
            ]
            fragments.append(
                {
                    "id": int(fragment.attrib["id"]),
                    "nodes": nodes,
                    "bonds": bonds,
                }
            )
        texts = [
            {
                "id": int(text.attrib["id"]),
                "p": tuple(float(part) for part in text.attrib["p"].split()),
                "runs": [
                    (
                        int(run.attrib["font"]),
                        float(run.attrib["size"]),
                        int(run.attrib["face"]),
                        int(run.attrib["color"]),
                        run.text or "",
                    )
                    for run in text.findall("s")
                ],
            }
            for text in page.findall("t")
        ]
        graphics = []
        for graphic in page.findall("graphic"):
            left, top, right, bottom = (
                float(part) for part in graphic.attrib["BoundingBox"].split()
            )
            represent = graphic.find("represent")
            if represent is None:
                raise ValueError("graphic without represent")
            graphics.append(
                {
                    "id": int(graphic.attrib["id"]),
                    "box": (left, top, right, bottom),
                    "type": graphic.attrib["GraphicType"],
                    "represents": (
                        int(represent.attrib["object"]),
                        represent.attrib["attribute"],
                    ),
                }
            )
        pages.append(
            {"fragments": fragments, "texts": texts, "graphics": graphics}
        )
    return {
        "fonts": sorted(
            (int(font.attrib["id"]), font.attrib["name"], 1252)
            for font in root.findall("fonttable/font")
            if font.attrib["charset"] == "iso-8859-1"
        ),
        "colors": [
            tuple(float(color.attrib[channel]) for channel in "rgb")
            for color in root.findall("colortable/color")
        ],
        "pages": pages,
    }


def _convert(direction: str, data: bytes) -> bytes:
    cargo = shutil.which("cargo")
    if cargo is None:
        raise RuntimeError("Cargo is required to build the conversion utility")
    result = subprocess.run(  # noqa: S603 - Local Cargo and fixed arguments, no shell.
        [cargo, "run", "--quiet", "--locked", "-p", "chemifree-providers",
         "--example", "cdx-convert", "--", direction],
        cwd=ROOT,
        input=data,
        capture_output=True,
        check=True,
    )  # fmt: skip
    return result.stdout


def main() -> None:
    """Compares the CDX reading with the CDXML reading, in both directions."""
    binary = _fixture()
    own = _Reader(binary).document()
    xml_text = _convert("to-cdxml", binary).decode("utf-8")
    if _xml_axes(xml_text) != own:
        raise RuntimeError(
            "CDXML output disagrees with the independent CDX reading"
        )
    again = _convert("to-cdx", xml_text.encode("utf-8"))
    if _Reader(again).document() != own:
        raise RuntimeError("CDX written from CDXML disagrees with the original")
    if again != binary:
        raise RuntimeError("CDX -> CDXML -> CDX changed the bytes")
    print(
        "Independent CDX comparison passed for graph, text, styles and colors."
    )


if __name__ == "__main__":
    main()
