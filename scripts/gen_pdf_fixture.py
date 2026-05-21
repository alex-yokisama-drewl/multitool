#!/usr/bin/env python3
"""Generate PDF fixtures for multitool-core tests.

Why this exists: pdfium-render is strict about PDF structure, so the basic
fixtures need correct cross-reference table byte offsets. Computing those by
hand is error-prone; this script does it deterministically. Encryption is
delegated to Ghostscript, which is the lowest-friction option that produces
a real, pdfium-rejected encrypted PDF.

Usage:
    python3 scripts/gen_pdf_fixture.py

Emits (re-run only when the fixture set changes):
    src-tauri/multitool-core/tests/fixtures/three-page.pdf    (3 empty pages)
    src-tauri/multitool-core/tests/fixtures/single-page.pdf   (1 empty page)
    src-tauri/multitool-core/tests/fixtures/encrypted.pdf     (single-page + password)
    src-tauri/multitool-core/tests/fixtures/corrupt.pdf       (truncated header)

Page objects have no /Contents entry; pdfium renders them as blank, which is
fine — these fixtures exist to exercise the binding, error paths, and the
page-iteration loop, not to validate rendering output.

Ghostscript >=9 must be on PATH for the encrypted fixture.
"""

from __future__ import annotations

import shutil
import subprocess
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


def encrypt_with_gs(source: Path, dest: Path) -> None:
    """Encrypt `source` with Ghostscript and write to `dest`.

    Both owner and user passwords are set, so pdfium will refuse to open
    the document without supplying one.
    """
    gs = shutil.which("gs")
    if gs is None:
        raise SystemExit("ghostscript ('gs') not found on PATH; cannot generate encrypted fixture")
    subprocess.run(
        [
            gs,
            "-q",
            "-dBATCH",
            "-dNOPAUSE",
            "-sDEVICE=pdfwrite",
            "-sOwnerPassword=multitool-test",
            "-sUserPassword=multitool-test",
            f"-sOutputFile={dest}",
            str(source),
        ],
        check=True,
    )


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    fixture_dir = repo_root / "src-tauri" / "multitool-core" / "tests" / "fixtures"
    fixture_dir.mkdir(parents=True, exist_ok=True)

    fixtures: list[tuple[str, bytes]] = [
        ("three-page.pdf", build_pdf(3)),
        ("single-page.pdf", build_pdf(1)),
        # Structurally valid PDF that pdfium loads OK but with zero pages —
        # used to exercise convert.rs's post-load empty check.
        ("zero-page.pdf", build_pdf(0)),
        # `corrupt.pdf` looks like a PDF until it abruptly isn't. pdfium's
        # parser rejects it as a format error, which is what we want for the
        # "ProcessingFailed" mapping test.
        ("corrupt.pdf", b"%PDF-1.4\nthis is not a valid PDF body\n%%EOF\n"),
    ]
    for name, data in fixtures:
        target = fixture_dir / name
        target.write_bytes(data)
        print(f"wrote {target} ({target.stat().st_size} bytes)")

    encrypted = fixture_dir / "encrypted.pdf"
    encrypt_with_gs(fixture_dir / "single-page.pdf", encrypted)
    print(f"wrote {encrypted} ({encrypted.stat().st_size} bytes)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
