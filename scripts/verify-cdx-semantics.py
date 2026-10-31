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


def _fixture(explicit: bool) -> bytes:
    # Independent source from public CDX field definitions: C(-1) double O.
    carbon = _object(0x8004, 3, _property(0x0200, struct.pack('<ii', -65536, 131072)) + _property(0x0421, b'\xff'))
    oxygen = _object(0x8004, 4, _property(0x0402, struct.pack('<h', 8)))
    bond = _object(0x8005, 5, _property(0x0604, struct.pack('<I', 3)) + _property(0x0605, struct.pack('<I', 4)) + _property(0x0600, struct.pack('<H', 2)))
    page = _object(0x8001, 1, _object(0x8003, 2, carbon + oxygen + bond))
    document = _object(0x8000, 9, page) if explicit else page
    return b'VjCD0100\x04\x03\x02\x01' + bytes(16) + document + b'\0\0'


def _check_framing(data: bytes, explicit: bool) -> None:
    """Independently check nested object ends and the final document terminator."""
    if data[:28] != b'VjCD0100\x04\x03\x02\x01' + bytes(16):
        raise AssertionError('bad CDX header')

    def body(offset: int) -> int:
        while True:
            if offset + 2 > len(data):
                raise AssertionError('missing object terminator')
            tag = struct.unpack_from('<H', data, offset)[0]
            offset += 2
            if tag == 0:
                return offset
            if tag & 0x8000:
                if offset + 4 > len(data):
                    raise AssertionError('truncated object ID')
                offset = body(offset + 4)
            else:
                if offset + 2 > len(data):
                    raise AssertionError('truncated property length')
                length = struct.unpack_from('<H', data, offset)[0]
                offset += 2
                if length == 0xffff:
                    if offset + 4 > len(data):
                        raise AssertionError('truncated extended length')
                    length = struct.unpack_from('<I', data, offset)[0]
                    offset += 4
                offset += length
                if offset > len(data):
                    raise AssertionError('truncated property')

    if explicit:
        if data[28:34] != struct.pack('<HI', 0x8000, 9):
            raise AssertionError('missing explicit document')
        offset = body(34)
        if data[offset:offset + 2] != b'\0\0':
            raise AssertionError('missing final document terminator')
        offset += 2
    else:
        offset = body(28)
    if offset != len(data):
        raise AssertionError('trailing CDX bytes')


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
        expected = ([(3, 6, -1, '2 -1'), (4, 8, 0, None)], [(5, 3, 4, '2')])
        for explicit in (False, True):
            cdx = root / 'independent.cdx'
            source = _fixture(explicit)
            _check_framing(source, explicit)
            cdx.write_bytes(source)
            xml = subprocess.run([str(binary), 'to-xml', str(cdx)], check=True, capture_output=True).stdout
            if _axes(xml) != expected:
                raise AssertionError(f'CDX to XML semantics differ: {_axes(xml)!r}')
            xml_path = root / 'source.cdxml'
            xml_path.write_bytes(xml)
            regenerated = root / 'regenerated.cdx'
            output = subprocess.run([str(binary), 'to-cdx', str(xml_path)], check=True, capture_output=True).stdout
            _check_framing(output, explicit)
            regenerated.write_bytes(output)
            compared = subprocess.run([str(binary), 'to-xml', str(regenerated)], check=True, capture_output=True).stdout
            if _axes(compared) != expected:
                raise AssertionError('XML to CDX graph differs from the independent expectations')
            if explicit:
                cdx.write_bytes(source[:-2])
                result = subprocess.run([str(binary), 'to-xml', str(cdx)], check=False, capture_output=True)
                if result.returncode == 0:
                    raise AssertionError('accepted an explicit document without its file terminator')
    print('Independent CDX framing and standard-library CDXML graph comparison passed')


if __name__ == '__main__':
    main()
