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


def _window(introduced: str | None, op: str, bound: str) -> str:
    """One introduced..bound window as an AND spec (`>=a,<b` or a single side)."""
    lo = f">={introduced}" if introduced else ""
    hi = f"{op}{bound}"
    return ",".join(part for part in (lo, hi) if part)


def _consume_event(introduced: str | None, ev: dict) -> tuple[str | None, str | None]:
    """Apply one OSV range event. Returns `(next_introduced, spec_or_none)`."""
    if "introduced" in ev:
        value = ev["introduced"]
        return (None if value == "0" else value), None
    if "fixed" in ev:
        return None, _window(introduced, "<", ev["fixed"])
    if "last_affected" in ev:
        return None, _window(introduced, "<=", ev["last_affected"])
    return introduced, None


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
            introduced, spec = _consume_event(introduced, ev)
            if spec:
                specs.append(spec)
        if introduced is not None:  # introduced with no fix yet → open-ended
            specs.append(f">={introduced}")
    for version in affected.get("versions", []) or []:
        specs.append(f"=={version}")
    # An advisory with neither ranges nor versions affects all versions.
    return sorted(set(specs))


def _osv_summary(data: dict) -> str:
    summary = data.get("summary") or data.get("details", "") or ""
    if not summary:
        return ""
    return summary.strip().splitlines()[0][:200]


def _osv_severity(data: dict) -> str | None:
    severity = None
    for item in data.get("severity", []) or []:
        severity = item.get("type") or severity
    return severity


def _pypi_advisories(data: dict) -> list[dict]:
    aliases = data.get("aliases", []) or []
    cves = [alias for alias in aliases if alias.startswith("CVE-")]
    summary = _osv_summary(data)
    severity = _osv_severity(data)
    advisories: list[dict] = []
    for aff in data.get("affected", []) or []:
        pkg = aff.get("package", {})
        if pkg.get("ecosystem") != "PyPI":
            continue
        name = pkg.get("name")
        if not name:
            continue
        advisories.append(
            {
                "id": data.get("id", ""),
                "package": name,
                "specs": _osv_ranges_to_specs(aff),
                "summary": summary,
                "aliases": cves,
                "severity": severity,
            }
        )
    return advisories


def from_osv(local_zip: str | None = None) -> list[dict]:
    blob = open(local_zip, "rb").read() if local_zip else _get(OSV_PYPI_ALL)
    advisories: list[dict] = []
    with zipfile.ZipFile(io.BytesIO(blob)) as zf:
        for name in zf.namelist():
            if not name.endswith(".json"):
                continue
            advisories.extend(_pypi_advisories(json.loads(zf.read(name))))
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


def _parse_args(args: list[str]) -> tuple[str, str, str | None]:
    out = "advisories.json"
    source = "osv"
    local_zip = None
    i = 0
    while i < len(args):
        nxt = args[i + 1] if i + 1 < len(args) else None
        if nxt is not None and args[i] == "--source":
            source = nxt
            i += 2
        elif nxt is not None and args[i] == "--zip":
            local_zip = nxt
            i += 2
        else:
            out = args[i]
            i += 1
    return out, source, local_zip


def _load(source: str, local_zip: str | None) -> tuple[list[dict], str]:
    if source == "osv":
        return from_osv(local_zip), OSV_PYPI_ALL
    return from_safety(), SAFETY_DB


def main() -> int:
    out, source, local_zip = _parse_args(sys.argv[1:])
    try:
        advisories, src_url = _load(source, local_zip)
    except Exception as exc:  # noqa: BLE001
        if source != "osv":
            raise
        print(f"OSV fetch failed ({exc}); falling back to safety-db.", file=sys.stderr)
        advisories, src_url = from_safety(), SAFETY_DB

    advisories.sort(key=lambda a: (a["package"], a["id"]))
    db = {"schema": "mollify-advisories/1", "source": src_url, "advisories": advisories}
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(db, fh, indent=0, sort_keys=True)
    print(f"Wrote {len(advisories)} advisories to {out} (source: {src_url})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
