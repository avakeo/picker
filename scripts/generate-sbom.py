#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Generates docs/sbom.cdx.json and docs/licenses.md from the lock files (T4-2).

The output depends only on Cargo.lock, package-lock.json, and the app
version, so running it twice gives the same files. `--check` compares the
committed files with fresh output and exits with 1 when they differ; CI runs
it so that a dependency change cannot leave the license list behind.

    uv run scripts/generate-sbom.py          # or: python3 scripts/generate-sbom.py
    uv run scripts/generate-sbom.py --check
"""

import argparse
import hashlib
import json
import subprocess
import sys
import tomllib
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SBOM_PATH = ROOT / "docs" / "sbom.cdx.json"
LICENSES_PATH = ROOT / "docs" / "licenses.md"


def app_version() -> str:
    manifest = tomllib.loads((ROOT / "src-tauri" / "Cargo.toml").read_text())
    return manifest["package"]["version"]


def component(kind: str, name: str, version: str, license_name: str | None) -> dict:
    return {
        "type": "library",
        "name": name,
        "version": version,
        "purl": f"pkg:{kind}/{name}@{version}",
        "licenses": [{"license": {"name": license_name or "NOASSERTION"}}],
    }


def cargo_components() -> dict:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT
        )
    )
    components = {}
    for package in metadata["packages"]:
        # Workspace crates have no source; they are this application.
        if package.get("source") is None:
            continue
        key = (package["name"], package["version"])
        components[key] = component(
            "cargo", package["name"], package["version"], package.get("license")
        )
    return components


def npm_components() -> dict:
    lock = ROOT / "package-lock.json"
    if not lock.exists():
        return {}
    components = {}
    for path, info in json.loads(lock.read_text()).get("packages", {}).items():
        # The root entry is this application; dev tools are not shipped.
        if not path or info.get("dev"):
            continue
        name = info.get("name") or path.split("node_modules/")[-1]
        version = info.get("version", "0.0.0")
        components[(name, version)] = component(
            "npm", name, version, info.get("license")
        )
    return components


def render() -> tuple[str, str, int]:
    components = {**cargo_components(), **npm_components()}
    ordered = [components[key] for key in sorted(components)]
    version = app_version()
    # The serial number follows the content: identical inputs give an
    # identical file, and any change gives a new serial number.
    digest = hashlib.sha256(
        json.dumps([version, ordered], sort_keys=True).encode()
    ).hexdigest()
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_OID, digest)}",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": "picker", "version": version},
            "tools": [{"name": "scripts/generate-sbom.py"}],
        },
        "components": ordered,
    }
    licenses: dict[str, set[str]] = {}
    for entry in ordered:
        licenses.setdefault(entry["licenses"][0]["license"]["name"], set()).add(
            entry["name"]
        )
    lines = [
        "# 依存ライセンス一覧",
        "",
        "`scripts/generate-sbom.py` で生成。モデルは別途アプリ内に表示する。",
        "",
    ]
    for name in sorted(licenses):
        lines += [f"## {name}", "", ", ".join(sorted(licenses[name])), ""]
    return (
        json.dumps(sbom, indent=2, ensure_ascii=False) + "\n",
        "\n".join(lines),
        len(ordered),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate the SBOM and license list.")
    parser.add_argument(
        "--check", action="store_true", help="fail if the committed files are outdated"
    )
    args = parser.parse_args()
    sbom, licenses, count = render()
    outputs = ((SBOM_PATH, sbom), (LICENSES_PATH, licenses))
    if args.check:
        stale = [
            str(path.relative_to(ROOT))
            for path, expected in outputs
            if not path.exists() or path.read_text() != expected
        ]
        if stale:
            print(
                f"outdated: {', '.join(stale)}. "
                "Run `uv run scripts/generate-sbom.py` and commit the result.",
                file=sys.stderr,
            )
            return 1
        print("SBOM and license list are current")
        return 0
    for path, content in outputs:
        path.write_text(content)
    print(f"{count} components -> {SBOM_PATH.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
