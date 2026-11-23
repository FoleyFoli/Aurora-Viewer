"""Run matched offline demos with exact archived viewer binaries on one macOS display."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

TMP = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
INSPECT = TMP / "metal-window"
CASES = [
    ("surface-cycle", "before"), ("surface-cycle", "after"),
    ("texture-cycle", "after"), ("texture-cycle", "before"),
    ("hair", "before"), ("hair", "after"),
    ("slope", "after"), ("slope", "before"),
    ("texture-cycle-noshadows", "after"),
]


def ui_state(log):
    state = "no toggle logged"
    if log.exists():
        for line in log.read_text(errors="replace").splitlines():
            if "interface: UI rendering feature " in line:
                state = line.rsplit("feature ", 1)[-1].strip()
    return state


def window_capture(pid, prefix, log):
    before = ui_state(log)
    dest = OUT / (prefix + "-window.png")
    try:
        wid = subprocess.check_output([INSPECT, "window", str(pid)], text=True, timeout=4).strip()
        if not wid.isdecimal():
            raise ValueError("Quartz did not identify the viewer window")
        subprocess.run(["/usr/sbin/screencapture", "-x", "-o", "-l", wid, dest],
                       capture_output=True, check=True, timeout=5)
        result = "captured " + dest.name
    except (subprocess.SubprocessError, OSError, ValueError) as err:
        result = f"window unavailable ({err})"
    with (OUT / "window-timing.tsv").open("a") as fp:
        print(prefix, pid, before, ui_state(log), result, sep="\t", file=fp)


def run_case(case, side):
    name = case + "-" + side
    cycle = "cycle" in case
    frames = [500, 780, 1080] if cycle else [780]
    with tempfile.TemporaryDirectory(prefix="aurora-capture-", dir=TMP) as tmp:
        root = Path(tmp)
        install = root / "install"
        install.mkdir()
        exe = install / "aurora-viewer"
        os.link(TMP / ("viewer-" + side) / "aurora-viewer", exe)
        marker = {
            "schema": 1,
            "feed": "http://127.0.0.1:1/feed.json",
            "public_key": "64b9f63e39ac482952f1ec5fb675b62b2331b272951e8292f5394dd84e099b7c",
        }
        (root / "aurora-update-test.json").write_text(json.dumps(marker))
        # Only the four first-launch floaters are closed; bars and normal UI remain enabled.
        prefs = dict.fromkeys(("show_people", "show_chat", "show_minimap", "show_perf"), False)
        if case == "hair":
            prefs["dof"] = {"enabled": True, "focus": "Camera", "f_number": 1.4, "transition": 0.1}
        (root / "profile").mkdir()
        (root / "profile" / "settings.json").write_text(json.dumps(prefs))
        env = {k: os.environ[k] for k in ("HOME", "PATH", "TMPDIR", "LANG", "LC_ALL", "USER") if k in os.environ}
        env.update(AURORA_DEMO="1", AURORA_UPDATE_TEST_ROOT=str(root), AURORA_AA="6",
                   AURORA_WINDOW_SIZE="960,560", AURORA_CAPTURE=str(root / "frame.png"),
                   AURORA_CAPTURE_FRAMES=",".join(map(str, frames)), AURORA_CAPTURE_EXIT="1",
                   AURORA_LOG_NAME="rendering-diag")
        if cycle:
            env.update(AURORA_DEMO_UI_FEATURE="cycle", AURORA_DEMO_HAIR_CARDS="12", AURORA_DEMO_DOF="1.4:3.3")
            if case.startswith("texture"):
                env["AURORA_DEMO_RENDER_TEXTURE"] = "1"
            if case.endswith("-noshadows"):
                env["AURORA_SHADOWS"] = "0"
        elif case == "hair":
            env.update(AURORA_DEMO_POS="133,129", AURORA_DEMO_HAIR_CARDS="12", AURORA_DEMO_CAMERA="close,479,380,40")
        elif case == "slope":
            env.update(AURORA_DEMO_POS="132,126", AURORA_DEMO_SOFT_SHADOWS="slope", AURORA_DEMO_SUN="27,45",
                       AURORA_SHADOWS="3", AURORA_SOFT_SHADOWS="1", AURORA_CONTACT_SHADOWS="1")
        (OUT / (name + "-settings.json")).write_text(json.dumps({"environment":
            {k: v for k, v in env.items() if k.startswith("AURORA_") and k not in {"AURORA_UPDATE_TEST_ROOT", "AURORA_CAPTURE"}},
            "profile": prefs}, indent=2) + "\n")
        log = root / "logs" / "rendering-diag.log"
        prefix = OUT / name
        paths = {f: root / (f"frame-{f}.png" if cycle else "frame.png") for f in frames}
        captured_os = set()
        print("Starting", name, flush=True)
        with (OUT / (name + "-console.log")).open("wb") as console:
            viewer = subprocess.Popen([exe, "--title", "Test rendering diagnostic", "--language", "en"],
                                      cwd=install, env=env, stdout=console, stderr=subprocess.STDOUT)
            try:
                until = time.monotonic() + 360
                while viewer.poll() is None:
                    if cycle and case == "surface-cycle":
                        for f in (500, 780):
                            if f not in captured_os and paths[f].exists():
                                captured_os.add(f)
                                window_capture(viewer.pid, f"{name}-{f}", log)
                    if time.monotonic() >= until:
                        raise TimeoutError(name + " exceeded six minutes")
                    time.sleep(0.12)
            finally:
                if viewer.poll() is None:
                    viewer.kill()
                viewer.wait()
                if log.exists():
                    shutil.copyfile(log, prefix.with_suffix(".log"))
                for f, p in paths.items():
                    if p.exists():
                        shutil.copyfile(p, OUT / f"{name}-{f}.png")
        if viewer.returncode or not log.exists() or not all(p.exists() for p in paths.values()):
            raise RuntimeError(f"{name}: exit {viewer.returncode}, missing log or frame")
        text = log.read_text(errors="replace")
        if cycle:
            off, on = "interface: UI rendering feature off", "interface: UI rendering feature on"
            if off not in text or on not in text[text.index(off):]:
                raise RuntimeError(name + ": viewer did not confirm the expected UI cycle")
        if "wgpu error" in text.lower():
            raise RuntimeError(name + ": wgpu reported a graphics error")


def main():
    OUT.mkdir(exist_ok=True)
    (OUT / "window-timing.tsv").write_text("capture\tpid\tui_before\tui_after\tresult\n")
    failures = []
    for case, side in CASES:
        try:
            run_case(case, side)
        except (OSError, RuntimeError, subprocess.SubprocessError) as err:
            failures.append(str(err))
    with (OUT / "pixels.tsv").open("w") as out:
        print("filename\twidth\theight\tnonblack\tpixels\tinterior_nonblack\tinterior_pixels", file=out)
        for p in sorted(OUT.glob("*.png")):
            try:
                stats = subprocess.check_output([INSPECT, "stats", p], text=True).strip()
                print(p.name, stats, sep="\t", file=out)
                if stats.split("\t")[2] == "0":
                    print("::warning::" + p.name + " is uniformly black")
            except (OSError, subprocess.SubprocessError) as err:
                failures.append(f"{p.name}: {err}")
    if failures:
        raise SystemExit("\n".join(failures))


if __name__ == "__main__":
    main()
