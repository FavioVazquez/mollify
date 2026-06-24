#!/usr/bin/env python3
"""Fetch Python vulnerability advisories and emit Mollify's normalized DB.

Mollify itself never touches the network — analysis reads the JSON this script
writes, so audits stay deterministic and offline. Run this (locally or in CI)
to refresh `.mollify/advisories.json`.

Sources (in preference order):
  1. OSV.dev official PyPI export (the GCS bucket dump, not the query API):
     https://osv-vulnerabilities.storage.googleapis.com/PyPI/all.zip
  2. pyup safety-db (fallback; community, less current):
     https://raw.githubusercontent.com/pyupio/safety-db/master/data/insecure_full.json

Output schema (`mollify-advisories/1`):
  {"schema": "mollify-advisories/1", "source": "...", "advisories": [
     {"id","package","specs":["<2.11.3", ...],"summary","aliases":["CVE-..."],"severity"}]}

Usage:
  python3 scripts/fetch-advisories.py [output.json] [--source osv|safety]
"""
from __future__ import annotations

import io
import json
import sys
import urllib.request
import zipfile

OSV_PYPI_ALL = "https://osv-vulnerabilities.storage.googleapis.com/PyPI/all.zip"
SAFETY_DB = (
    "https://raw.githubusercontent.com/pyupio/safety-db/master/data/insecure_full.json"
)


def _get(url: str, attempts: int = 4) -> bytes:
    """Fetch a URL, retrying on short/incomplete reads (proxies can truncate
    large streams). Reads in chunks and verifies Content-Length when present."""
    last: Exception | None = None
    for _ in range(attempts):
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "mollify-fetch-advisories"}
            )
            with urllib.request.urlopen(req, timeout=180) as resp:  # noqa: S310
                expected = resp.headers.get("Content-Length")
                buf = io.BytesIO()
                while True:
                    chunk = resp.read(1 << 16)
                    if not chunk:
                        break
                    buf.write(chunk)
                data = buf.getvalue()
            if expected and len(data) != int(expected):
                raise OSError(f"short read: {len(data)}/{expected} bytes")
            return data
        except Exception as exc:  # noqa: BLE001
            last = exc
    raise last  # type: ignore[misc]


def _osv_ranges_to_specs(affected: dict) -> list[str]:
    """Convert one OSV `affected` entry's ranges/versions into spec strings.

    An OSV ECOSYSTEM/SEMVER range is a sorted list of `introduced`/`fixed`
    events. Each introduced..fixed window becomes one AND spec (">=a,<b");
    an open window becomes ">=a". Explicit `versions` become "==v" specs.
    """
    specs: list[str] = []
    for rng in affected.get("ranges", []):
        introduced = None
        for ev in rng.get("events", []):
            if "introduced" in ev:
                introduced = ev["introduced"]
                if introduced == "0":
                    introduced = None
            elif "fixed" in ev:
                lo = f">={introduced}" if introduced else ""
                hi = f"<{ev['fixed']}"
                specs.append(",".join(p for p in (lo, hi) if p))
                introduced = None
            elif "last_affected" in ev:
                lo = f">={introduced}" if introduced else ""
                hi = f"<={ev['last_affected']}"
                specs.append(",".join(p for p in (lo, hi) if p))
                introduced = None
        if introduced is not None:  # introduced with no fix yet → open-ended
            specs.append(f">={introduced}")
    for v in affected.get("versions", []) or []:
        specs.append(f"=={v}")
    # An advisory with neither ranges nor versions affects all versions.
    return sorted(set(specs))


def from_osv(local_zip: str | None = None) -> list[dict]:
    blob = open(local_zip, "rb").read() if local_zip else _get(OSV_PYPI_ALL)
    advisories: list[dict] = []
    with zipfile.ZipFile(io.BytesIO(blob)) as zf:
        for name in zf.namelist():
            if not name.endswith(".json"):
                continue
            data = json.loads(zf.read(name))
            aliases = data.get("aliases", []) or []
            summary = data.get("summary") or data.get("details", "") or ""
            summary = summary.strip().splitlines()[0][:200] if summary else ""
            severity = None
            for s in data.get("severity", []) or []:
                severity = s.get("type") or severity
            for aff in data.get("affected", []) or []:
                pkg = aff.get("package", {})
                if pkg.get("ecosystem") != "PyPI":
                    continue
                name_ = pkg.get("name")
                if not name_:
                    continue
                advisories.append(
                    {
                        "id": data.get("id", ""),
                        "package": name_,
                        "specs": _osv_ranges_to_specs(aff),
                        "summary": summary,
                        "aliases": [a for a in aliases if a.startswith("CVE-")],
                        "severity": severity,
                    }
                )
    return advisories


def from_safety() -> list[dict]:
    data = json.loads(_get(SAFETY_DB))
    advisories: list[dict] = []
    for pkg, entries in data.items():
        if pkg == "$meta":
            continue
        for e in entries:
            specs = [s.replace(" ", "") for s in e.get("specs", []) if s]
            advisories.append(
                {
                    "id": e.get("id", e.get("cve", "")) or f"SAFETY-{pkg}",
                    "package": pkg,
                    "specs": specs,
                    "summary": (e.get("advisory", "") or "").strip()[:200],
                    "aliases": [e["cve"]] if e.get("cve") else [],
                    "severity": None,
                }
            )
    return advisories


def main() -> int:
    out = "advisories.json"
    source = "osv"
    local_zip = None
    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--source" and i + 1 < len(args):
            source = args[i + 1]
            i += 2
        elif args[i] == "--zip" and i + 1 < len(args):
            local_zip = args[i + 1]
            i += 2
        else:
            out = args[i]
            i += 1

    try:
        advisories = from_osv(local_zip) if source == "osv" else from_safety()
        src_url = OSV_PYPI_ALL if source == "osv" else SAFETY_DB
    except Exception as exc:  # noqa: BLE001
        if source == "osv":
            print(f"OSV fetch failed ({exc}); falling back to safety-db.", file=sys.stderr)
            advisories = from_safety()
            src_url = SAFETY_DB
        else:
            raise

    advisories.sort(key=lambda a: (a["package"], a["id"]))
    db = {"schema": "mollify-advisories/1", "source": src_url, "advisories": advisories}
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(db, fh, indent=0, sort_keys=True)
    print(f"Wrote {len(advisories)} advisories to {out} (source: {src_url})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
