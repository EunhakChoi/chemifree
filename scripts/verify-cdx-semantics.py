"""Compare independently framed CDX graph semantics with a CDXML reader.

Use through the shared resource wrapper after building the cdx_reference example.
"""
from __future__ import annotations

import pathlib
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET


def _object(tag: int, ident: int, body: bytes) -> bytes:
    return struct.pack('<HI', tag, ident) + body + b'\0\0'


def _property(tag: int, value: bytes) -> bytes:
    return struct.pack('<HH', tag, len(value)) + value


def _fixture() -> bytes:
    # Independent source from public CDX field definitions: C(-1) double O.
    carbon = _object(0x8004, 3, _property(0x0200, struct.pack('<ii', -65536, 131072)) + _property(0x0421, b'\xff'))
    oxygen = _object(0x8004, 4, _property(0x0402, struct.pack('<h', 8)))
    bond = _object(0x8005, 5, _property(0x0604, struct.pack('<I', 3)) + _property(0x0605, struct.pack('<I', 4)) + _property(0x0600, struct.pack('<H', 2)))
    return b'VjCD0100\x04\x03\x02\x01' + bytes(16) + _object(0x8001, 1, _object(0x8003, 2, carbon + oxygen + bond)) + b'\0\0'


def _axes(text: bytes) -> tuple:
    root = ET.fromstring(text)
    nodes = root.findall('./page/fragment/n')
    bonds = root.findall('./page/fragment/b')
    return (
        [(int(n.attrib['id']), int(n.get('Element', '6')), int(n.get('Charge', '0')), n.get('p')) for n in nodes],
        [(int(b.attrib['id']), int(b.attrib['B']), int(b.attrib['E']), b.get('Order', '1')) for b in bonds],
    )


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit('usage: verify-cdx-semantics.py /absolute/path/to/cdx_reference')
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='chemifree-cdx-') as tmp:
        root = pathlib.Path(tmp)
        cdx = root / 'independent.cdx'
        cdx.write_bytes(_fixture())
        xml = subprocess.run([str(binary), 'to-xml', str(cdx)], check=True, capture_output=True).stdout
        expected = ([(3, 6, -1, '2 -1'), (4, 8, 0, None)], [(5, 3, 4, '2')])
        if _axes(xml) != expected:
            raise AssertionError(f'CDX to XML semantics differ: {_axes(xml)!r}')
        xml_path = root / 'source.cdxml'
        xml_path.write_bytes(xml)
        regenerated = root / 'regenerated.cdx'
        regenerated.write_bytes(subprocess.run([str(binary), 'to-cdx', str(xml_path)], check=True, capture_output=True).stdout)
        compared = subprocess.run([str(binary), 'to-xml', str(regenerated)], check=True, capture_output=True).stdout
        if _axes(compared) != expected:
            raise AssertionError('XML to CDX graph differs from the independent expectations')
    print('Independent CDX framing and standard-library CDXML graph comparison passed')


if __name__ == '__main__':
    main()
