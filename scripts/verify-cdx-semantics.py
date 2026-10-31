"""Compare independently framed CDX graph semantics with a CDXML reader.

Use through the shared resource wrapper after building cdx_reference.
"""

from __future__ import annotations

import pathlib
import struct
import subprocess
import sys
import tempfile
from typing import TypeAlias
import xml.etree.ElementTree

NodeAxis: TypeAlias = tuple[int, int, int, str | None]
BondAxis: TypeAlias = tuple[int, int, int, str]
TextAxis: TypeAlias = tuple[int, int, float, int, str]
FontAxis: TypeAlias = tuple[int, str, str]


def _object(tag: int, ident: int, body: bytes) -> bytes:
    return struct.pack("<HI", tag, ident) + body + b"\0\0"


def _property(tag: int, value: bytes) -> bytes:
    return struct.pack("<HH", tag, len(value)) + value


def _fixture(explicit: bool) -> bytes:
    # Independent source from public CDX field definitions: C(-1) double O.
    carbon = _object(
        0x8004,
        3,
        _property(0x0200, struct.pack("<ii", -65536, 131072))
        + _property(0x0421, b"\xff"),
    )
    oxygen = _object(0x8004, 4, _property(0x0402, struct.pack("<h", 8)))
    bond = _object(
        0x8005,
        5,
        _property(0x0604, struct.pack("<I", 3))
        + _property(0x0605, struct.pack("<I", 4))
        + _property(0x0600, struct.pack("<H", 2)),
    )
    # Public CDX font table and styled-string fields, including Latin-1 é.
    fonts = _property(
        0x0100, struct.pack("<HHHHH", 1, 1, 3, 1252, 5) + b"Arial"
    )
    styled = struct.pack("<HHHHHH", 1, 0, 3, 1, 240, 2) + b"Caf\xe9"
    text = _object(0x8006, 6, _property(0x0700, styled))
    page = _object(0x8001, 1, _object(0x8003, 2, carbon + oxygen + bond) + text)
    body = fonts + page
    document = _object(0x8000, 9, body) if explicit else body
    return b"VjCD0100\x04\x03\x02\x01" + bytes(16) + document + b"\0\0"


def _check_framing(data: bytes, explicit: bool) -> None:
    """Independently check nested object ends and the document terminator."""
    if data[:28] != b"VjCD0100\x04\x03\x02\x01" + bytes(16):
        raise AssertionError("bad CDX header")

    def body(offset: int) -> int:
        while True:
            if offset + 2 > len(data):
                raise AssertionError("missing object terminator")
            tag = struct.unpack_from("<H", data, offset)[0]
            offset += 2
            if tag == 0:
                return offset
            if tag & 0x8000:
                if offset + 4 > len(data):
                    raise AssertionError("truncated object ID")
                offset = body(offset + 4)
            else:
                if offset + 2 > len(data):
                    raise AssertionError("truncated property length")
                length = struct.unpack_from("<H", data, offset)[0]
                offset += 2
                if length == 0xFFFF:
                    if offset + 4 > len(data):
                        raise AssertionError("truncated extended length")
                    length = struct.unpack_from("<I", data, offset)[0]
                    offset += 4
                offset += length
                if offset > len(data):
                    raise AssertionError("truncated property")

    if explicit:
        if data[28:34] != struct.pack("<HI", 0x8000, 9):
            raise AssertionError("missing explicit document")
        offset = body(34)
        if data[offset : offset + 2] != b"\0\0":
            raise AssertionError("missing final document terminator")
        offset += 2
    else:
        offset = body(28)
    if offset != len(data):
        raise AssertionError("trailing CDX bytes")


def _axes(
    text: bytes,
) -> tuple[list[NodeAxis], list[BondAxis], list[TextAxis], list[FontAxis]]:
    # This XML is produced from the local authored fixture by the local codec.
    root = xml.etree.ElementTree.fromstring(text)  # noqa: S314
    nodes = root.findall("./page/fragment/n")
    bonds = root.findall("./page/fragment/b")
    return (
        [
            (
                int(node.attrib["id"]),
                int(node.get("Element", "6")),
                int(node.get("Charge", "0")),
                node.get("p"),
            )
            for node in nodes
        ],
        [
            (
                int(bond.attrib["id"]),
                int(bond.attrib["B"]),
                int(bond.attrib["E"]),
                bond.get("Order", "1"),
            )
            for bond in bonds
        ],
        [
            (
                int(run.attrib["font"]),
                int(run.attrib["face"]),
                float(run.attrib["size"]),
                int(run.attrib["color"]),
                run.text or "",
            )
            for run in root.findall("./page/t/s")
        ],
        [
            (
                int(font.attrib["id"]),
                font.attrib["name"],
                font.attrib["charset"],
            )
            for font in root.findall("./fonttable/font")
        ],
    )


def _run(
    binary: pathlib.Path,
    action: str,
    source: pathlib.Path,
    *,
    check: bool = True,
) -> subprocess.CompletedProcess[bytes]:
    # The caller supplies the local reference executable; no shell is used.
    return subprocess.run(  # noqa: S603
        [str(binary), action, str(source)], check=check, capture_output=True
    )


def main() -> None:
    """Compare independently framed binary graphs with decoded XML graphs."""
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: verify-cdx-semantics.py /absolute/path/to/cdx_reference"
        )
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="chemifree-cdx-") as tmp:
        root = pathlib.Path(tmp)
        expected: tuple[
            list[NodeAxis], list[BondAxis], list[TextAxis], list[FontAxis]
        ] = (
            [(3, 6, -1, "2 -1"), (4, 8, 0, None)],
            [(5, 3, 4, "2")],
            [(3, 1, 12.0, 2, "Café")],
            [(3, "Arial", "iso-8859-1")],
        )
        for explicit in (False, True):
            cdx = root / "independent.cdx"
            source = _fixture(explicit)
            _check_framing(source, explicit)
            cdx.write_bytes(source)
            xml = _run(binary, "to-xml", cdx).stdout
            if _axes(xml) != expected:
                raise AssertionError(
                    f"CDX to XML semantics differ: {_axes(xml)!r}"
                )
            xml_path = root / "source.cdxml"
            xml_path.write_bytes(xml)
            regenerated = root / "regenerated.cdx"
            output = _run(binary, "to-cdx", xml_path).stdout
            _check_framing(output, explicit)
            regenerated.write_bytes(output)
            compared = _run(binary, "to-xml", regenerated).stdout
            if _axes(compared) != expected:
                raise AssertionError(
                    "XML to CDX graph differs from the independent expectations"
                )
            if explicit:
                cdx.write_bytes(source[:-2])
                result = _run(binary, "to-xml", cdx, check=False)
                if result.returncode == 0:
                    raise AssertionError(
                        "accepted an explicit document without its "
                        "file terminator"
                    )
    sys.stdout.write(
        "Independent CDX framing and CDXML graph/text/style comparison passed\n"
    )


if __name__ == "__main__":
    main()
