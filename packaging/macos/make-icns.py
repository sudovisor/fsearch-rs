#!/usr/bin/env python3
"""Generate a macOS .icns icon file from a source image."""

import io
import os
import struct
import sys
from PIL import Image

def make_icns(src_path: str, dst_path: str):
    img = Image.open(src_path).convert('RGBA')
    # Standard modern macOS ICNS chunk types for PNG data:
    # icp4: 16x16
    # icp5: 32x32
    # icp6: 64x64
    # ic07: 128x128
    # ic08: 256x256
    # ic09: 512x512
    sizes = [
        (b'icp4', 16),
        (b'icp5', 32),
        (b'icp6', 64),
        (b'ic07', 128),
        (b'ic08', 256),
        (b'ic09', 512),
    ]
    body = bytearray()
    for tag, size in sizes:
        resized = img.resize((size, size), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        resized.save(buf, format='PNG')
        png_bytes = buf.getvalue()
        chunk_len = len(png_bytes) + 8
        body.extend(tag)
        body.extend(struct.pack('>I', chunk_len))
        body.extend(png_bytes)

    header = b'icns' + struct.pack('>I', len(body) + 8)
    os.makedirs(os.path.dirname(os.path.abspath(dst_path)), exist_ok=True)
    with open(dst_path, 'wb') as f:
        f.write(header + body)
    print(f"Generated {dst_path} ({len(header + body)} bytes)")

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print("Usage: make-icns.py <input.png> <output.icns>")
        sys.exit(1)
    make_icns(sys.argv[1], sys.argv[2])
