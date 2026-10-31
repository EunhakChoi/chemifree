"""Check a CDXML rewrite with Python's independent standard-library XML reader.

Run through the shared build wrapper from the repository root. This tests a
bounded fixture; it does not claim general CDXML compatibility.
"""

# ruff: noqa: T201 - This standalone verification command prints its result.

from __future__ import annotations

import pathlib
import shutil
import subprocess
from typing import TypeAlias, TypedDict
import xml.etree.ElementTree

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "crates/providers/tests/data/cdxml/multi-page.cdxml"

Point: TypeAlias = tuple[float, ...] | None
Atom: TypeAlias = tuple[int, int, int, int, Point]
Bond: TypeAlias = tuple[int, int, int, str]
Fragment: TypeAlias = tuple[int, list[Atom], list[Bond]]
StyleRun: TypeAlias = tuple[int, float, int, str | None]
Text: TypeAlias = tuple[int, Point, list[StyleRun]]
Graphic: TypeAlias = tuple[int, Point, str, list[tuple[int, str]]]
Page: TypeAlias = tuple[int, list[Fragment], list[Text], list[Graphic]]


class Axes(TypedDict):
    """The independent semantic axes compared in the authored fixture."""

    root: str
    fonts: list[tuple[int, str, str]]
    colors: list[tuple[float, ...]]
    pages: list[Page]


def _coordinates(value: str | None) -> tuple[float, ...] | None:
    return (
        None if value is None else tuple(float(part) for part in value.split())
    )


def _axes(source: str) -> Axes:
    # The local authored fixture has no entities or resolved external resources.
    root = xml.etree.ElementTree.fromstring(source)  # noqa: S314
    pages = []
    for page in root.findall("page"):
        fragments = []
        for fragment in page.findall("fragment"):
            atoms = [
                (
                    int(node.attrib["id"]),
                    int(node.get("Element", "6")),
                    int(node.get("Charge", "0")),
                    int(node.get("Isotope", "0")),
                    _coordinates(node.get("p")),
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
            fragments.append((int(fragment.attrib["id"]), atoms, bonds))
        texts = [
            (
                int(text.attrib["id"]),
                _coordinates(text.get("p")),
                [
                    (
                        int(run.attrib["font"]),
                        float(run.attrib["size"]),
                        int(run.attrib["face"]),
                        run.text,
                    )
                    for run in text.findall("s")
                ],
            )
            for text in page.findall("t")
        ]
        graphics = [
            (
                int(graphic.attrib["id"]),
                _coordinates(graphic.get("BoundingBox")),
                graphic.attrib["GraphicType"],
                [
                    (int(ref.attrib["object"]), ref.attrib["attribute"])
                    for ref in graphic.findall("represent")
                ],
            )
            for graphic in page.findall("graphic")
        ]
        pages.append((int(page.attrib["id"]), fragments, texts, graphics))
    return {
        "root": root.tag,
        "fonts": [
            (
                int(font.attrib["id"]),
                font.attrib["name"],
                font.attrib["charset"],
            )
            for font in root.findall("fonttable/font")
        ],
        "colors": [
            tuple(float(color.attrib[channel]) for channel in ("r", "g", "b"))
            for color in root.findall("colortable/color")
        ],
        "pages": pages,
    }


def _expected(first_page: int) -> Axes:
    return {
        "root": "CDXML",
        "fonts": [(3, "Serif", "utf-8")],
        "colors": [(1.0, 1.0, 1.0), (0.0, 0.0, 0.0)],
        "pages": [
            (
                first_page,
                [
                    (
                        2,
                        [(3, 6, 0, 0, (1.5, -2.0)), (4, 8, -1, 18, (2.0, 4.0))],
                        [(5, 3, 4, "2")],
                    )
                ],
                [(6, (10.0, 20.0), [(3, 12.0, 1, "\u03b1 & \u03b2")])],
                [(10, (0.0, 2.0, 3.0, 4.0), "Line", [(4, "Charge")])],
            ),
            (7, [(8, [(9, 7, 0, 0, None)], [])], [], []),
        ],
    }


def main() -> None:
    """Run a forced provider rewrite and independently compare its semantics."""
    source = FIXTURE.read_text(encoding="utf-8")
    if _axes(source) != _expected(1):
        raise RuntimeError(
            "The independently read fixture disagrees with expected semantics"
        )
    cargo = shutil.which("cargo")
    if cargo is None:
        raise RuntimeError("Cargo is required to build the fixture utility")
    result = subprocess.run(  # noqa: S603 - Local Cargo and fixed arguments, no shell.
        [
            cargo,
            "run",
            "--quiet",
            "--locked",
            "-p",
            "chemifree-providers",
            "--example",
            "cdxml-rewrite-fixture",
        ],
        cwd=ROOT,
        input=source,
        text=True,
        capture_output=True,
        check=True,
    )
    if _axes(result.stdout) != _expected(11):
        raise RuntimeError(
            "The rewritten CDXML disagrees with the expected semantic axes"
        )
    print("Independent XML comparison passed for graph, scene, text and style.")


if __name__ == "__main__":
    main()
