"""Run exact archived viewers with native Metal validation or a shader experiment."""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TEMP = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
MODE = sys.argv[3]
SOURCE = Path.cwd() / "crates/aurora-render/src/shaders"
OUT.mkdir(exist_ok=True)


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
        env = {k: v for k, v in os.environ.items() if not k.startswith(("AURORA_", "MTL_"))}
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
            RUST_LOG="info,wgpu_hal=debug,wgpu_core=warn,naga=warn,reqwest=warn",
        )
        if MODE in {"api", "shader"}:
            env.update(MTL_DEBUG_LAYER="1", AURORA_GPU_VALIDATION="1")
        if MODE == "shader":
            env.update(MTL_SHADER_VALIDATION="1", MTL_SHADER_VALIDATION_REPORT_TO_STDERR="1")
        if override:
            compiled_path = str(SOURCE.parent.parent).encode()
            if compiled_path not in exe.read_bytes():
                raise RuntimeError("Archived executable does not contain the expected shader source directory")
            shutil.copy2(override, SOURCE / "object.wgsl")
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
                result = subprocess.run(
                    [exe, "--title", f"Test Metal {name}", "--language", "en"],
                    cwd=install, env=env, stdout=console, stderr=subprocess.STDOUT, timeout=600,
                ).returncode
            except subprocess.TimeoutExpired:
                result = "timeout"
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
        return {"case": name, "exit": result}


if MODE in {"api", "shader"}:
    results = [run_viewer("after", "candidate"), run_viewer("before", "upstream")]
else:
    override_name = "object-hoisted.wgsl" if MODE == "hoisted" else "object-shared-shadow.wgsl"
    override_path = Path.cwd() / ".github/qa" / override_name
    results = [run_viewer("after", "baked"), run_viewer("after", "override", override_path)]

(OUT / "results.json").write_text(json.dumps(results, indent=2))
with (OUT / "pixels.tsv").open("w") as output:
    print("image\twidth\theight\tnonblack\tpixels\tinterior_nonblack\tinterior_pixels", file=output)
    for image in sorted(OUT.glob("*.png")):
        stats = subprocess.check_output([TEMP / "metal-window", "stats", image], text=True).strip()
        print(image.name, stats, sep="\t", file=output)
        print(image.name, stats, sep="\t", flush=True)

for console in sorted(OUT.glob("*-console.log")):
    for line in console.read_text(errors="replace").splitlines():
        if any(word in line.lower() for word in ("validation error", "failed assertion", "shader validation", "invalid resource", "out of bounds", "fault", "panic")):
            print(console.name, line[:1000], flush=True)
