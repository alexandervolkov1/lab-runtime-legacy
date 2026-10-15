"""Run one committed proof profile, retaining original command logs and hashes."""
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
PROFILE = sys.argv[1]
assert PROFILE in ("dev", "release")
RUN = sys.argv[2]
assert RUN.replace("-", "").isalnum()
OUT = ROOT / "target/render-2a" / RUN
OUT.mkdir(parents=True, exist_ok=False)
source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
assert not subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all", "--", "apps", "crates", "scripts", "Cargo.toml", "Cargo.lock"], cwd=ROOT).strip(), "Commit executable inputs first"
manifest = "scripts/support/wgpu-proof/Cargo.toml"
binary = ROOT / "scripts/support/wgpu-proof/target" / ("debug" if PROFILE == "dev" else "release") / "lab-wgpu-proof.exe"
commands = [
    ("fmt", ["cargo", "fmt", "--manifest-path", manifest, "--", "--check"]),
    ("clippy", ["cargo", "clippy", "--manifest-path", manifest, "--locked", "--offline", "--profile", PROFILE, "--", "-D", "warnings"]),
    ("build", ["cargo", "build", "--manifest-path", manifest, "--locked", "--offline", "--profile", PROFILE]),
    ("gpu", [str(binary), "--controller", str(OUT / "gpu")]),
]
for name, command in commands:
    start = time.monotonic()
    utc = datetime.datetime.now(datetime.timezone.utc).isoformat()
    print("START", name, flush=True)
    with (OUT / (name + ".log")).open("wb") as stream:
        result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
    receipt = dict(command=command, source_head=source, profile=PROFILE, started_utc=utc,
                   elapsed_seconds=round(time.monotonic()-start, 3), exit_code=result.returncode,
                   log_sha256=hashlib.sha256((OUT/(name+".log")).read_bytes()).hexdigest())
    if name == "gpu":
        receipt["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        receipt["binary_bytes"] = binary.stat().st_size
    (OUT/(name+".json")).write_text(json.dumps(receipt, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(receipt), flush=True)
    if result.returncode:
        print((OUT/(name+".log")).read_text(encoding="utf-8", errors="replace")[-12000:], flush=True)
        sys.exit(1)
comparisons = []
for wgpu in sorted((OUT/"gpu/frames").glob("g*-f*.json")):
    a = json.loads(wgpu.read_text())
    b = json.loads((OUT/"gpu/glow"/wgpu.name).read_text())
    assert a["pixels"]["pass"] and b["pixels"]["pass"]
    for key in ("title", "scalar", "sample", "scalar_rect", "vertices", "indices", "mesh_fnv64", "updates", "free", "texture_id"):
        assert a[key] == b[key], (wgpu.name, key, a[key], b[key])
    comparisons.append(dict(frame=wgpu.name, cpu_geometry_equal=True, labels_equal=True, actual_pixels_both_pass=True))
assert len(comparisons) == 24
(OUT/"comparison.json").write_text(json.dumps(comparisons, indent=2)+"\n", encoding="utf-8")
print("PASS: 24 identical CPU geometries; 48 real GPU readbacks; independently tolerated pixels", flush=True)
