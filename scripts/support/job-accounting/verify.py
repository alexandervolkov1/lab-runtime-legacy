"""Check finite MP-1 native experiments; exact byte totals remain observations.

Usage: python verify.py CPU_CONSOLE_DIRECTORY CPU_WINDOWED_DIRECTORY [GRAPH_DIRECTORY ...]
No OS counter is substituted for allocation/exit/CloseHandle receipts.
"""
import json
from pathlib import Path
import sys

MIB = 1024 * 1024
TOLERANCE = 65536  # Small receipt/JSON heap changes, never applied to cap readback.


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def common(path):
    data = load(path / "accounting.json")
    assert data["retired_objects"] == {"jobs": 0, "processes": 0, "threads": 0}, path
    assert data["diagnostic_port_CloseHandle_succeeded"] is True, path
    samples = [s for s in data["samples"] if "snapshot" in s]
    for sample in samples:
        s = sample["snapshot"]
        assert s["extended_bytes"] == 144 and s["accounting_bytes"] == 48, path
        assert s["flags"] == 0x2308 and s["active_process_limit"] == 1, path
        assert s["process_limit"] == data["process_limit"] and s["job_limit"] == data["job_limit"], path
        assert s["original_process_member"] is True, path
    before = samples[0]
    assert before["phase"] == "suspended_before_ResumeThread", path
    assert before["snapshot"]["active_processes"] == before["snapshot"]["total_processes"] == 1, path
    assert samples[-1]["phase"] == "original_process_signaled_exit", path
    return data


def cpu(directory, windowed):
    summary = []
    for path in sorted(directory.glob("*/accounting.json")):
        data = common(path.parent)
        assert data["exit_code"] == 0 and not data["termination_requested"], path
        phases = {s["child"]["phase"]: s for s in data["samples"] if "child" in s}
        ordered = [s for s in data["samples"] if "child" in s]
        init = phases["initialized"]["snapshot"]
        members = init["members"]
        helpers = [m for m in members if m.get("image", "").lower().endswith("\\conhost.exe")]
        assert len(members) == (1 if windowed else 2), path
        assert len(helpers) == (0 if windowed else 1), path
        reserved = phases["reserved"]
        assert reserved["child"]["detail"]["reserved"] == 128 * MIB, path
        assert reserved["child"]["detail"]["next_region"][0] == 0x2000, path
        assert abs(reserved["snapshot"]["K32GetProcessMemoryInfo"]["private_usage"] - init["K32GetProcessMemoryInfo"]["private_usage"]) <= TOLERANCE, path
        denial = phases["allocation_denied"]
        detail = denial["child"]["detail"]
        assert detail["operation"] == "VirtualAlloc MEM_COMMIT" and detail["error"] == 1455, path
        assert detail["next_region"][0] == 0x2000, path
        before = ordered[ordered.index(denial) - 1]["snapshot"]
        after = denial["snapshot"]
        live_before = before["K32GetProcessMemoryInfo"]["private_usage"]
        live_after = after["K32GetProcessMemoryInfo"]["private_usage"]
        assert abs(live_before - live_after) <= TOLERANCE, path
        messages = [m[0] for m in denial["messages"]]
        # This is an observation check for this Windows build, not a guarantee
        # that Windows delivers every hard-limit completion message on all hosts.
        assert 9 in messages or 10 in messages, path
        if 10 in messages:
            assert after["peak_job"] > data["job_limit"], path
            assert abs((after["K32GetProcessMemoryInfo"]["peak_pagefile_usage"] - live_before) - detail["request"]) <= TOLERANCE, path
        else:
            assert after["peak_process"] <= data["process_limit"], path
        decommit = phases["decommitted"]
        assert decommit["child"]["detail"]["next_region"][0] == 0x2000, path
        assert live_after - decommit["snapshot"]["K32GetProcessMemoryInfo"]["private_usage"] >= detail["committed"] - TOLERANCE, path
        assert "released" in phases, path
        summary.append({"case": path.parent.name, "windowed": windowed, "members": len(members),
                        "before_denial_private": live_before, "after_denial_private": live_after,
                        "request": detail["request"], "peak_process": after["peak_process"],
                        "peak_job": after["peak_job"], "limit_messages": messages})
    assert len(summary) == 5, directory
    return summary


def graphical(directory):
    data = common(directory)
    assert data["exit_code"] == 92 and data["termination_requested"], directory
    assert data["process_limit"] == data["job_limit"] == 16 * MIB, directory
    assert "dx12 found no adapters" in load(directory / "failure.json")["error"], directory
    first = next(s for s in data["samples"] if s.get("failure"))
    s = first["snapshot"]
    helper = next(m for m in s["members"] if m.get("image", "").lower().endswith("\\conhost.exe"))
    helper_commit = helper["memory"]["private_usage"]
    # Equality is checked for this retained observation, not inferred for history.
    assert s["peak_job"] - s["peak_process"] == helper_commit, directory
    assert s["peak_process"] == s["K32GetProcessMemoryInfo"]["peak_pagefile_usage"], directory
    assert s["peak_job"] > data["job_limit"], directory
    assert any(m[0] == 10 for sample in data["samples"] for m in sample.get("messages", [])), directory
    return {"graphical": str(directory), "peak_process": s["peak_process"],
            "peak_job": s["peak_job"], "conhost_private": helper_commit,
            "current_child_private": s["K32GetProcessMemoryInfo"]["private_usage"], "exit": data["exit_code"]}


if __name__ == "__main__":
    console, windowed, *graphs = map(Path, sys.argv[1:])
    print(json.dumps({"cpu": cpu(console, False) + cpu(windowed, True),
                      "graphical": [graphical(p) for p in graphs], "passed": True}, indent=2))
