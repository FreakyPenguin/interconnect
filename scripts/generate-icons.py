#!/usr/bin/env python3
"""Regenerate the checked-in app icons from docs/logo.svg (requires ImageMagick)."""
from pathlib import Path
import shutil
import struct
import subprocess


ROOT = Path(__file__).resolve().parent.parent
SIZES = (16, 32, 48, 64, 128, 256, 512, 1024)
ICNS_TYPES = {
    16: (b"icp4",),
    32: (b"icp5", b"ic11"),
    64: (b"icp6", b"ic12"),
    128: (b"ic07",),
    256: (b"ic08", b"ic13"),
    512: (b"ic09", b"ic14"),
    1024: (b"ic10",),
}


def main():
    converter = shutil.which("magick") or shutil.which("convert")
    if converter is None:
        raise SystemExit("Install ImageMagick to regenerate icons; normal builds use checked-in assets.")
    output = ROOT / "assets/icons"
    output.mkdir(parents=True, exist_ok=True)
    chunks = []
    for size in SIZES:
        icon = output / f"interconnect-{size}.png"
        subprocess.run([
            converter, "-background", "none", "-density", "192",
            str(ROOT / "docs/logo.svg"), "-resize", f"{size}x{size}",
            "-strip", f"PNG32:{icon}",
        ], check=True)
        png = icon.read_bytes()
        for kind in ICNS_TYPES.get(size, ()):
            chunks.append(kind + struct.pack(">I", len(png) + 8) + png)
    # Modern macOS accepts PNG representations in an ICNS container. Keeping
    # this portable avoids requiring iconutil/macOS to update the logo.
    payload = b"".join(chunks)
    (output / "interconnect.icns").write_bytes(
        b"icns" + struct.pack(">I", len(payload) + 8) + payload
    )
    print(f"Generated {len(SIZES)} PNG sizes and interconnect.icns in {output}")


if __name__ == "__main__":
    main()
