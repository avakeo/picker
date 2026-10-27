#!/usr/bin/env python3
import json, subprocess, uuid
from pathlib import Path

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1"], cwd=root))
components = {}
for package in metadata["packages"]:
    if package.get("source") is None:
        continue
    key = (package["name"], package["version"])
    components[key] = {
        "type": "library",
        "name": package["name"],
        "version": package["version"],
        "purl": f"pkg:cargo/{package['name']}@{package['version']}",
        "licenses": [{"license": {"name": package.get("license") or "NOASSERTION"}}],
    }
lock = root / "package-lock.json"
if lock.exists():
    data = json.loads(lock.read_text())
    for path, info in data.get("packages", {}).items():
        if not path or info.get("dev"):
            continue
        name = info.get("name") or path.split("node_modules/")[-1]
        version = info.get("version", "0.0.0")
        components[(name, version)] = {
            "type": "library",
            "name": name,
            "version": version,
            "purl": f"pkg:npm/{name}@{version}",
            "licenses": [{"license": {"name": info.get("license") or "NOASSERTION"}}],
        }
sbom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, 'https://github.com/anomalyco/picker')}",
    "version": 1,
    "metadata": {
        "component": {"type": "application", "name": "picker", "version": "0.1.0"},
        "tools": [{"name": "scripts/generate-sbom.py"}],
    },
    "components": [components[key] for key in sorted(components)],
}
out = root / "docs" / "sbom.cdx.json"
out.write_text(json.dumps(sbom, indent=2, ensure_ascii=False) + "\n")
licenses = {}
for component in components.values():
    name = component["licenses"][0]["license"]["name"]
    licenses.setdefault(name, set()).add(component["name"])
lines = ["# 依存ライセンス一覧", "", "`scripts/generate-sbom.py` で生成。モデルは別途アプリ内に表示する。", ""]
for name in sorted(licenses):
    lines.append(f"## {name}")
    lines.append("")
    lines.append(", ".join(sorted(licenses[name])))
    lines.append("")
(root / "docs" / "licenses.md").write_text("\n".join(lines))
print(f"{len(components)} components -> {out}")
