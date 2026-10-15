"""Reproduce B3B4's native boundary conflict without running GL or a child.

The positive control type-checks the unchanged B2B2 native source. The negative
control applies Workbench's existing unsafe prohibition to the same source.
Neither result qualifies an isolated renderer, its pixels, or process memory.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]
QUALIFIED = "82d8ad12e3cf71f7b389a21e94e456b214da9e0d"


def run(arguments):
    return subprocess.run(arguments, cwd=ROOT, capture_output=True, check=False)


def checked(arguments):
    result = run(arguments)
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    return result.stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("dev", "release"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    toolchain = checked(["rustc", "-vV"]).decode()
    if "host: x86_64-pc-windows-msvc" not in toolchain or "release: 1.95.0\n" not in toolchain:
        raise RuntimeError("Existing Rust 1.95.0 x64 MSVC qualification toolchain required")
    (output / "rustc.txt").write_text(toolchain, encoding="utf-8")
    inventory = json.loads(
        (ROOT / "docs/developer/private-readonly-ipc-evidence/source-inputs.json").read_bytes()
    )
    inputs = [item for item in inventory["files"] if not item["path"].startswith("docs/")]
    queries = [f"{revision}:{item['path']}" for item in inputs for revision in (QUALIFIED, "HEAD")]
    batch = subprocess.run(["git", "cat-file", "--batch"], cwd=ROOT,
                           input=("\n".join(queries) + "\n").encode(), capture_output=True, check=True).stdout
    blobs = []
    offset = 0
    for _ in queries:
        end = batch.index(b"\n", offset)
        _, kind, size = batch[offset:end].split()
        if kind != b"blob":
            raise RuntimeError("Expected an exact inventoried Git blob")
        offset = end + 1
        end = offset + int(size)
        blobs.append(batch[offset:end])
        offset = end + 1
    identities = []
    for index, item in enumerate(inputs):
        path = item["path"]
        baseline, current = blobs[2 * index:2 * index + 2]
        raw = (ROOT / path).read_bytes()
        if digest(baseline) != item["git_blob_sha256"] or baseline != current:
            raise RuntimeError(f"Qualified Git source changed: {path}")
        if raw.replace(b"\r\n", b"\n") != current.replace(b"\r\n", b"\n"):
            raise RuntimeError(f"Qualified worktree content changed: {path}")
        identities.append({"path": path, "git_sha256": digest(current), "raw_sha256": digest(raw),
                           "raw_matches_historical": digest(raw) == item["tested_windows_bytes_sha256"]})
    build = run([
        "cargo", "build", "-p", "lab-workbench", "--locked", "--profile", args.profile,
        "--message-format=json",
    ])
    (output / "cargo.jsonl").write_bytes(build.stdout)
    (output / "cargo-stderr.txt").write_bytes(build.stderr)
    if build.returncode:
        raise RuntimeError("Workbench artifact control build failed; see cargo-stderr.txt")
    records = [json.loads(line) for line in build.stdout.splitlines() if line]
    artifacts = [record for record in records if record.get("reason") == "compiler-artifact"]
    metadata = json.loads(checked(["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"]))
    package = next(p for p in metadata["packages"] if p["name"] == "lab-workbench")
    requirements = {
        d["name"].replace("-", "_"): set(d["features"])
        for d in package["dependencies"] if d["kind"] is None
    }
    native = {"glutin": "0.32.3", "glutin_winit": "0.5.0", "winit": "0.30.13", "raw_window_handle": "0.6.2"}
    externs = []
    for name in dict.fromkeys([*requirements, *native]):
        candidates = [a for a in artifacts if a["target"]["name"] == name
                      and requirements.get(name, set()).issubset(a["features"])]
        files = {f for a in candidates for f in a["filenames"] if f.endswith(".rlib")}
        if len(files) != 1:
            raise RuntimeError(f"Ambiguous artifact: {name}")
        if name in native and any(not a["package_id"].endswith("@" + native[name]) for a in candidates):
            raise RuntimeError(f"Unreviewed native version: {name}")
        externs.extend(["--extern", f"{name}={files.pop()}"])
    for path in sorted({p for r in records if r.get("reason") == "build-script-executed" for p in r["linked_paths"]}):
        externs.extend(["-L", path])
    source = ROOT / "apps/lab-workbench/src"
    modules = []
    for name in ("client", "dispatcher", "external", "model", "ownership", "presentation", "recovery", "storage"):
        path = source / f"{name}.rs"
        if not path.exists():
            path = source / name / "mod.rs"
        modules.append(f'#[path = "{path.as_posix()}"] mod {name};')
    wrapper = "\n".join([
        "//! B3B4 compile-only compatibility probe; no native execution.",
        "#![deny(unsafe_code)]", "#![allow(dead_code, unused_imports)]", *modules,
        f'#[path = "{source.as_posix()}/gui/rebuild.rs"] mod rebuild;',
        f'mod gui {{ #[path = "{source.as_posix()}/gui/readonly_v2/mod.rs"] pub(crate) mod readonly_v2; }}',
        "use gui::readonly_v2;",
        f'#[path = "{source.as_posix()}/gui/readonly_v2/plot_fixtures.rs"] mod plot_fixtures;',
        f'#[path = "{ROOT.as_posix()}/scripts/support/native-glow-smoke.rs"] mod native_glow;',
        "fn main() { native_glow::run(); }", "",
    ])
    flags = (["-C", "debug-assertions=yes", "-C", "overflow-checks=yes"] if args.profile == "dev"
             else ["-C", "opt-level=3", "-C", "debug-assertions=no", "-C", "overflow-checks=no"])
    directory = "debug" if args.profile == "dev" else "release"
    results = []
    for name, lint, expected in (("b2b2-control", "deny", 0), ("workbench-forbid", "forbid", 1)):
        path = output / f"{name}.rs"
        path.write_text(wrapper.replace("#![deny(unsafe_code)]", f"#![{lint}(unsafe_code)]"), encoding="utf-8")
        command = ["rustc", "--edition=2024", "-D", "warnings", "--emit=metadata", "--error-format=json",
                   str(path), *externs, *flags, "-L", f"dependency={ROOT / 'target' / directory / 'deps'}",
                   "-o", str(output / f"{name}.rmeta")]
        result = run(command)
        (output / f"{name}.jsonl").write_bytes(result.stderr)
        diagnostics = [json.loads(line) for line in result.stderr.splitlines() if line.startswith(b"{")]
        codes = sorted({d["code"]["code"] for d in diagnostics if d.get("code")})
        if (expected == 0 and result.returncode != 0) or (expected == 1 and (result.returncode == 0 or "E0453" not in codes)):
            raise RuntimeError(f"Unexpected control result for {name}; see diagnostics")
        results.append({"name": name, "command": command, "exit_code": result.returncode,
                        "diagnostic_codes": codes, "source_sha256": digest(path.read_bytes()),
                        "diagnostics_sha256": digest(result.stderr)})
    report = {
        "profile": args.profile, "source_head": checked(["git", "rev-parse", "HEAD"]).decode().strip(),
        "qualified_source": QUALIFIED, "unchanged_qualified_inputs": len(identities),
        "historical_raw_byte_matches": sum(item["raw_matches_historical"] for item in identities),
        "worktree_comparison": "Git blobs exact; worktree matches after CRLF/LF normalization",
        "input_identities": identities, "controls": results,
        "meaning": "Compile-only conflict reproduced; no child, GPU, memory, or lifecycle qualification",
        "activation_qualified": False,
        "audit_script_sha256": digest(Path(__file__).read_bytes()),
    }
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "input_identities"}, indent=2))


if __name__ == "__main__":
    main()
