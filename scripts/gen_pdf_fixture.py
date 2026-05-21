#!/usr/bin/env python3
"""Generate minimal hand-rolled PDF fixtures for multitool-core tests.

Why this exists: pdfium-render is strict about PDF structure, so the fixtures
need correct cross-reference table byte offsets. Computing those by hand is
error-prone; this script does it deterministically. The generated PDFs are
checked into `src-tauri/multitool-core/tests/fixtures/` — re-run only if the
fixture set changes.

Usage:
    python3 scripts/gen_pdf_fixture.py

Currently emits:
    src-tauri/multitool-core/tests/fixtures/three-page.pdf

Pages are intentionally empty (no /Contents entry); pdfium renders them as
blank, which is fine for smoke-testing the binding + page-count read path.
"""

from __future__ import annotations

import sys
from pathlib import Path


def build_pdf(num_pages: int) -> bytes:
    """Build a PDF with N empty US-Letter pages and a correct xref table."""
    objects: list[bytes] = []
    # obj 1: catalog
    objects.append(b"<< /Type /Catalog /Pages 2 0 R >>")
    # obj 2: pages tree
    kids = " ".join(f"{3 + i} 0 R" for i in range(num_pages))
    objects.append(
        f"<< /Type /Pages /Kids [{kids}] /Count {num_pages} >>".encode("ascii")
    )
    # obj 3..N+2: page objects (no /Contents — pdfium renders blank)
    for _ in range(num_pages):
        objects.append(
            b"<< /Type /Page /Parent 2 0 R "
            b"/MediaBox [0 0 612 792] /Resources << >> >>"
        )

    # Binary-marker comment in the header so PDF tooling treats it as binary.
    body = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")

    offsets: list[int] = [0]  # index 0 is reserved for the free entry
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(body))
        body.extend(f"{i} 0 obj\n".encode("ascii"))
        body.extend(obj)
        body.extend(b"\nendobj\n")

    xref_offset = len(body)
    body.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
    body.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        body.extend(f"{offset:010d} 00000 n \n".encode("ascii"))

    body.extend(
        (
            f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
            f"startxref\n{xref_offset}\n%%EOF\n"
        ).encode("ascii")
    )
    return bytes(body)


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    fixture_dir = repo_root / "src-tauri" / "multitool-core" / "tests" / "fixtures"
    fixture_dir.mkdir(parents=True, exist_ok=True)

    target = fixture_dir / "three-page.pdf"
    target.write_bytes(build_pdf(3))
    print(f"wrote {target} ({target.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
