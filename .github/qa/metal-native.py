"""Isolate Metal failures with exact archived viewers and matched controls."""
import hashlib
import json
import os
import math
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

TEMP = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
MODE = sys.argv[3]
if MODE not in {"ordinary", "metal-api-only", "wgpu-only", "validation-both", "shadow-source-control", "shadow-graph-bypass", "occlusion-off", "dof-off", "pcss-single-loop"}:
    raise ValueError(f"Unknown validation mode: {MODE}")
SOURCE = Path.cwd() / "crates/aurora-render/src/shaders"
OUT.mkdir(exist_ok=True)


def collect_native(name, pid, started):
    # The relative window starts just before this launch. The PID attributes
    # entries to this viewer even if the two cases finish close together.
    time.sleep(2)
    window = f"{math.ceil(time.monotonic() - started) + 1}s"
    proc = subprocess.run(
        ["/usr/bin/log", "show", "--last", window, "--style", "compact", "--info",
         "--predicate", f"processID == {pid}"],
        text=True, capture_output=True, timeout=90,
    )
    (OUT / f"{name}-native.log").write_text(proc.stdout)
    (OUT / f"{name}-native-stderr.log").write_text(proc.stderr)
    return {
        "pid": pid, "query_exit": proc.returncode,
        "native_rows": len(re.findall(rf"(?m)^.*aurora-viewer\[{pid}:", proc.stdout)),
    }


def run_viewer(side, name, override=None):
    with tempfile.TemporaryDirectory(prefix="aurora-metal-", dir=TEMP) as temp:
        root = Path(temp)
        install = root / "install"
        install.mkdir()
        exe = install / "aurora-viewer"
        shutil.copy2(TEMP / f"viewer-{side}/aurora-viewer", exe)
        marker = {
            "schema": 1,
            "feed": "http://127.0.0.1:1/feed.json",
            "public_key": "64b9f63e39ac482952f1ec5fb675b62b2331b272951e8292f5394dd84e099b7c",
        }
        (root / "aurora-update-test.json").write_text(json.dumps(marker))
        (root / "profile").mkdir()
        prefs = dict.fromkeys(("show_people", "show_chat", "show_minimap", "show_perf"), False)
        (root / "profile/settings.json").write_text(json.dumps(prefs))
        env = {k: os.environ[k] for k in ("HOME", "PATH", "TMPDIR", "LANG", "LC_ALL", "USER") if k in os.environ}
        env.update(
            AURORA_DEMO="1",
            AURORA_UPDATE_TEST_ROOT=str(root),
            AURORA_AA="6",
            AURORA_WINDOW_SIZE="960,560",
            AURORA_DEMO_HAIR_CARDS="12",
            AURORA_DEMO_DOF="1.4:3.3",
            AURORA_CAPTURE=str(root / "frame.png"),
            AURORA_CAPTURE_FRAMES="500,780",
            AURORA_CAPTURE_EXIT="1",
            AURORA_DEMO_UI_FEATURE="off",
            AURORA_LOG_NAME="metal-native",
            RUST_BACKTRACE="1",
            RUST_LOG="info",
        )
        if MODE == "dof-off":
            del env["AURORA_DEMO_DOF"]
        if MODE in {"metal-api-only", "validation-both"}:
            env["MTL_DEBUG_LAYER"] = "1"
        if MODE in {"wgpu-only", "validation-both"}:
            env["AURORA_GPU_VALIDATION"] = "1"
        elif MODE == "occlusion-off":
            env["AURORA_NO_OCCLUSION"] = "1"
        if override:
            compiled_path = str(SOURCE.parent.parent).encode()
            if compiled_path not in exe.read_bytes():
                raise RuntimeError("Archived executable does not contain the expected shader source directory")
            if MODE == "shadow-graph-bypass":
                shutil.copy2(override, SOURCE / "common.wgsl")
            expected = {
                "pcss-single-loop": "2776eef6cc3974c52c7347c4e1a15f4eed8ce93619ac68969eb7fcf5c5c40405",
                "shadow-graph-bypass": "10914b6531b5473a0c793421f2e61fd4ee3c75f4ee52281567d56c76e87ecf3b",
                "shadow-source-control": "33a3070f6e044399697172750afb19625dcfe0637cb301fcabc09c22d7517667",
            }[MODE]
            if hashlib.sha256((SOURCE / "common.wgsl").read_bytes()).hexdigest() != expected:
                raise RuntimeError("Unexpected shader fixture")
            env["AURORA_DEMO_WATER_SHADERS"] = "1"
            checksums = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in SOURCE.glob("*.wgsl")}
            (OUT / f"{name}-shaders.json").write_text(json.dumps(checksums, indent=2))
        settings = {k: v for k, v in env.items() if k.startswith(("AURORA_", "MTL_"))}
        settings.pop("AURORA_UPDATE_TEST_ROOT", None)
        settings.pop("AURORA_CAPTURE", None)
        (OUT / f"{name}-settings.json").write_text(json.dumps(settings, indent=2))
        result = None
        print(f"Starting {name}", flush=True)
        with (OUT / f"{name}-console.log").open("wb") as console:
            try:
                started = time.monotonic()
                process = subprocess.Popen(
                    [exe, "--title", f"Test Metal {name}", "--language", "en"],
                    cwd=install, env=env, stdout=console, stderr=subprocess.STDOUT,
                )
                result = process.wait(timeout=600)
            except subprocess.TimeoutExpired:
                result = "timeout"
                process.kill()
                process.wait()
            finally:
                for path in root.glob("frame-*.png"):
                    shutil.copy2(path, OUT / f"{name}-{path.name}")
                log = root / "logs/metal-native.log"
                if log.exists():
                    shutil.copy2(log, OUT / f"{name}.log")
        console = (OUT / f"{name}-console.log").read_text(errors="replace")
        if override and "using baked source" in console:
            raise RuntimeError("The source override fell back to the baked shader")
        print(f"Finished {name}, exit={result}", flush=True)
        return {"case": name, "exit": result, **collect_native(name, process.pid, started)}


# The isolation case disables directional-map sampling only for the candidate.
# The upstream control keeps its embedded shaders.
override_path = None
if MODE == "shadow-graph-bypass":
    override_path = Path.cwd() / ".github/qa/common-directional-graph-bypass.wgsl"
elif MODE in {"shadow-source-control", "pcss-single-loop"}:
    override_path = SOURCE / "common.wgsl"
results = [run_viewer("after", "candidate", override_path), run_viewer("before", "upstream")]

(OUT / "results.json").write_text(json.dumps(results, indent=2))
with (OUT / "pixels.tsv").open("w") as output:
    print("image\twidth\theight\tnonblack\tpixels\tinterior_nonblack\tinterior_pixels\tinterior_rgb_bins", file=output)
    for image in sorted(OUT.glob("*.png")):
        stats = subprocess.check_output([TEMP / "metal-window", "stats", image], text=True).strip()
        print(image.name, stats, sep="\t", file=output)
        print(image.name, stats, sep="\t", flush=True)

for console in sorted(OUT.glob("*-console.log")):
    for line in console.read_text(errors="replace").splitlines():
        if any(word in line.lower() for word in ("validation error", "failed assertion", "shader validation", "invalid resource", "out of bounds", "fault", "panic")):
            print(console.name, line[:1000], flush=True)
