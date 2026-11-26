"""Checks, before a release is published, that every Windows program in it carries a code signature.

    python3 tools/release/check_signed.py <dist folder>

release.yml runs it in `publish` when the Windows files were signed through SignPath: every
MisterDownloadManager-*.exe in the folder, and every .exe inside its .zip files, must have an
Authenticode signature (a PKCS#7 entry in the PE file's certificate table). SignPath's jobs
check the signatures themselves on Windows (.github/actions/signpath); this is the last check on
the files that are actually published, so an unsigned file that got there by any other way
(a stale or leftover artifact) stops the release. Whether the signature is valid is not checked
here (no Windows on this runner); a file without one fails.
"""

import pathlib
import struct
import sys
import zipfile

WIN_CERT_TYPE_PKCS_SIGNED_DATA = 0x0002


def signed(data: bytes) -> bool:
    """Whether a PE file's certificate table holds a PKCS#7 signature."""
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("not a PE file")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + {0x10B: 96, 0x20B: 112}[magic]
    # The certificate table is data directory 4; its address is a file offset.
    offset, size = struct.unpack_from("<II", data, directories + 4 * 8)
    if not offset or size < 8 or offset + size > len(data):
        return False
    length, _revision, kind = struct.unpack_from("<IHH", data, offset)
    return kind == WIN_CERT_TYPE_PKCS_SIGNED_DATA and 8 < length <= size


def main() -> int:
    dist = pathlib.Path(sys.argv[1])
    checked, unsigned = 0, []
    for path in sorted(dist.iterdir()):
        if path.suffix.lower() == ".exe" and path.name.startswith("MisterDownloadManager-"):
            checked += 1
            if not signed(path.read_bytes()):
                unsigned.append(path.name)
        elif path.suffix.lower() == ".zip":
            with zipfile.ZipFile(path) as z:
                for entry in z.namelist():
                    if entry.lower().endswith(".exe"):
                        checked += 1
                        if not signed(z.read(entry)):
                            unsigned.append(f"{path.name}: {entry}")
    for name in unsigned:
        print(f"not signed: {name}", file=sys.stderr)
    print(f"{checked} Windows programs checked, {len(unsigned)} without a signature")
    if not checked:
        print("no Windows program found", file=sys.stderr)
        return 1
    return 1 if unsigned else 0


if __name__ == "__main__":
    sys.exit(main())
