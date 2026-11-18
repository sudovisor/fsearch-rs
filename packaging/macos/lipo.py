#!/usr/bin/env python3
"""Combine x86_64 and arm64 Mach-O binaries into a Universal 2 (fat) binary."""

import os
import struct
import sys

def align_to(offset: int, align: int) -> int:
    mask = (1 << align) - 1
    return (offset + mask) & ~mask

def lipo_universal2(x86_path: str, arm64_path: str, out_path: str):
    with open(x86_path, 'rb') as f:
        data_x86 = f.read()
    with open(arm64_path, 'rb') as f:
        data_arm = f.read()

    # Mach-O constants
    FAT_MAGIC = 0xcafebabe
    CPU_TYPE_X86_64 = 0x01000007
    CPU_SUBTYPE_X86_64_ALL = 0x00000003
    CPU_TYPE_ARM64 = 0x0100000c
    CPU_SUBTYPE_ARM64_ALL = 0x00000000

    # Apple Silicon requires 16KB (2^14 = 16384) page alignment
    align = 14
    header_size = 8 + 2 * 20  # 8-byte fat_header + 2 * 20-byte fat_arch = 48 bytes

    offset_x86 = align_to(header_size, align)
    offset_arm = align_to(offset_x86 + len(data_x86), align)

    fat_hdr = struct.pack('>II', FAT_MAGIC, 2)
    arch_x86 = struct.pack('>IIIII', CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64_ALL, offset_x86, len(data_x86), align)
    arch_arm = struct.pack('>IIIII', CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64_ALL, offset_arm, len(data_arm), align)

    os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)
    with open(out_path, 'wb') as f:
        f.write(fat_hdr)
        f.write(arch_x86)
        f.write(arch_arm)
        # Pad up to offset_x86
        pad1 = offset_x86 - f.tell()
        f.write(b'\x00' * pad1)
        f.write(data_x86)
        # Pad up to offset_arm
        pad2 = offset_arm - f.tell()
        f.write(b'\x00' * pad2)
        f.write(data_arm)

    os.chmod(out_path, 0o755)
    print(f"Created Universal 2 binary: {out_path} ({os.path.getsize(out_path)} bytes)")

if __name__ == '__main__':
    if len(sys.argv) < 4:
        print("Usage: lipo.py <x86_64_binary> <arm64_binary> <output_universal_binary>")
        sys.exit(1)
    lipo_universal2(sys.argv[1], sys.argv[2], sys.argv[3])
