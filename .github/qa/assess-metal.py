"""Assess captures from both archived executables and the logs of their own processes."""
import csv
import json
import os
import re
import sys
from pathlib import Path

GPU_ERROR = re.compile(
    r"wgpu error|validation error|failed assertion|MTLCommandBufferErrorDomain|"
    r"GPU[ \t]+Timeout[ \t]+Error|kIOGPUCommandBufferCallbackErrorTimeout|SubmissionsIgnored|"
    r"command[ -]?buffer[^\n]*(?:abort|fail)|abort[^\n]*command[ -]?buffer|"
    r"GPU device lost|panicked at",
    re.IGNORECASE,
)


def assess(directory, run, pixels, revision):
    name = run["case"]
    evidence = (directory / f"{name}.log").read_text(errors="replace")
    native = (directory / f"{name}-native.log").read_text(errors="replace")
    console = (directory / f"{name}-console.log").read_text(errors="replace")
    position = re.search(r"\bframe 780 captured to ", evidence)
    states = re.findall(r"interface: UI rendering feature (on|off)\b", evidence[:position.start()] if position else "")
    incomplete = []
    if not re.search(rf"\+[^\n]*g{re.escape(revision[:8])}[0-9a-f]* starting ", evidence):
        incomplete.append("revision")
    if not re.search(r"GPU: .+\(Metal,", evidence):
        incomplete.append("Metal adapter")
    if not position or states[-1:] != ["off"]:
        incomplete.append("UI-hidden frame 780")
    if run["query_exit"] != 0 or run["native_rows"] == 0:
        incomplete.append("PID-scoped native log")
    if not pixels or (pixels["width"], pixels["height"]) != (960, 560) or pixels["interior_pixels"] <= 0:
        incomplete.append("960x560 capture")
    visible = bool(pixels and pixels["interior_pixels"] > 0
                   and pixels["interior_nonblack"] / pixels["interior_pixels"] >= 0.5
                   and pixels["interior_rgb_bins"] >= 16)
    errors = [line for log in (native, console) for line in log.splitlines() if GPU_ERROR.search(line)]
    return {"pixels": pixels, "visible_world": visible, "gpu_errors": len(errors), "examples": errors[:6],
            "missing_evidence": incomplete, "pid": run["pid"], "exit": run["exit"],
            "passed": visible and not errors and run["exit"] == 0 and not incomplete}


def main():
    directory = Path(sys.argv[1])
    mode = sys.argv[2]
    revisions = {"candidate": os.environ["AFTER"], "upstream": os.environ["BEFORE"]}
    with (directory / "pixels.tsv").open() as stream:
        pixels = {row.pop("image"): {k: int(v) for k, v in row.items()}
                  for row in csv.DictReader(stream, delimiter="\t")}
    runs = json.loads((directory / "results.json").read_text())
    if [r["case"] for r in runs] != ["candidate", "upstream"]:
        raise RuntimeError("Missing or out-of-order candidate/upstream measurements")
    verdicts = {r["case"]: assess(directory, r, pixels.get(f"{r['case']}-frame-780.png"), revisions[r["case"]])
                for r in runs}
    (directory / "assessment.json").write_text(json.dumps({"mode": mode, "cases": verdicts}, indent=2) + "\n")
    with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as summary:
        print("| Mode | Build | Frame 780 shows world | GPU errors | Exit | Missing evidence |", file=summary)
        print("|---|---|---|---|---|---|", file=summary)
        for case, result in verdicts.items():
            print(f"| {mode} | {case} | {result['visible_world']} | {result['gpu_errors']} | "
                  f"{result['exit']} | {', '.join(result['missing_evidence'])} |", file=summary)
    return 0 if all(v["passed"] for v in verdicts.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
