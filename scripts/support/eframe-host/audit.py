"""Record the independent Windows graph and verify exact registry archive bytes."""
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
OUT = ROOT / "target/eframe-1/dependencies"
OUT.mkdir(parents=True, exist_ok=True)
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--manifest-path", str(HERE / "Cargo.toml"),
    "--locked", "--offline", "--format-version", "1",
    "--filter-platform", "x86_64-pc-windows-msvc"], text=True))
(OUT / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
old = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
new = tomllib.loads((HERE / "Cargo.lock").read_text())["package"]
old_keys = {(p["name"], p["version"]): p for p in old}
new_keys = {(p["name"], p["version"]): p for p in new}
shared = set(old_keys) & set(new_keys)
assert all(old_keys[k].get("checksum") == new_keys[k].get("checksum") for k in shared)
for name in ("egui", "epaint", "egui-wgpu"):
    assert [p["version"] for p in new if p["name"] == name] == ["0.36.2"]
nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
records = []
for package in metadata["packages"]:
    if package["id"] not in nodes:
        continue
    directory = Path(package["manifest_path"]).parent
    licenses = []
    source_files = 0
    archive_hash = None
    if package["source"]:
        archive = directory.parents[2] / "cache" / directory.parent.name / (directory.name + ".crate")
        archive_hash = hashlib.sha256(archive.read_bytes()).hexdigest()
        assert archive_hash == new_keys[(package["name"], package["version"])]["checksum"], archive
        with tarfile.open(archive) as tar:
            for member in tar:
                if not member.isfile():
                    continue
                relative = Path(*Path(member.name).parts[1:])
                data = tar.extractfile(member).read()
                assert (directory / relative).read_bytes() == data, (directory, relative)
                source_files += 1
                if any(word in relative.name.upper() for word in ("LICENSE", "COPYING", "NOTICE")):
                    licenses.append({"path": str(relative), "sha256": hashlib.sha256(data).hexdigest()})
        assert package["license"] or package["license_file"], package["name"]
    records.append({"name": package["name"], "version": package["version"],
                    "license": package["license"], "license_file": package["license_file"],
                    "features": nodes[package["id"]]["features"], "archive_sha256": archive_hash,
                    "verified_source_files": source_files, "license_files": licenses})
report = {"target": "x86_64-pc-windows-msvc", "root_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
          "proof_lock_sha256": hashlib.sha256((HERE / "Cargo.lock").read_bytes()).hexdigest(),
          "shared_exact_packages": len(shared),
          "added_to_independent_lock": sorted([list(k) for k in set(new_keys) - set(old_keys)]),
          "removed_from_independent_lock": sorted([list(k) for k in set(old_keys) - set(new_keys)]),
          "packages": sorted(records, key=lambda p: (p["name"], p["version"]))}
(OUT / "inventory.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
tree = subprocess.check_output(["cargo", "tree", "--manifest-path", str(HERE / "Cargo.toml"),
                               "--locked", "--offline", "--target", report["target"], "-e", "features"], text=True)
(OUT / "features.txt").write_text(tree, encoding="utf-8")
print(json.dumps({"verified_packages": len(records), "shared_exact": len(shared),
                  "licenses": sorted({p["license"] or "license-file" for p in records}),
                  "added": report["added_to_independent_lock"]}, indent=2))

