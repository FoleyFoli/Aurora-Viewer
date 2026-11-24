"""Gate two ordinary Metal captures from the exact newly built viewer.

Native API/shader validation changes the Metal path and is intentionally absent
here. A separate instrumented diagnostic cannot replace either production run.
"""
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


INHERITED_ENV = ("HOME", "PATH", "TMPDIR", "LANG", "LC_ALL", "USER")
CASES = ("surface-cycle", "hair-close")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def case_settings(case):
    prefs = dict.fromkeys(("show_people", "show_chat", "show_minimap", "show_perf"), False)
    environment = {
        "AURORA_DEMO": "1",
        "AURORA_AA": "6",
        "AURORA_WINDOW_SIZE": "960,560",
        "AURORA_DEMO_HAIR_CARDS": "12",
        "AURORA_CAPTURE_FRAMES": "780",
        "AURORA_CAPTURE_EXIT": "1",
        "AURORA_LOG_NAME": "final-metal",
        "RUST_LOG": "info",
    }
    if case == "surface-cycle":
        # The old failure's default camera and fixed-distance lens. The UI
        # goes off at 600; capture/exit at 780 precedes the cycle's on at 900.
        environment.update(AURORA_DEMO_DOF="1.4:3.3", AURORA_DEMO_UI_FEATURE="cycle")
    elif case == "hair-close":
        # Exact diagnostics/hair-after-settings.json camera and Camera-focus
        # profile, with the UI disabled at 600 for a world-only capture.
        # Do not add AURORA_DEMO_DOF: it would replace this Camera focus.
        environment.update(
            AURORA_DEMO_POS="133,129",
            AURORA_DEMO_CAMERA="close,479,380,40",
            AURORA_DEMO_UI_FEATURE="off",
        )
        prefs["dof"] = {"enabled": True, "focus": "Camera", "f_number": 1.4, "transition": 0.1}
    else:
        raise ValueError(f"Unknown Metal capture case: {case}")
    return environment, prefs


def viewer_environment(case, root):
    # Match the old failure helper exactly: do not inherit runner overlays,
    # validation flags, wgpu logging, shader overrides or existing profiles.
    env = {key: os.environ[key] for key in INHERITED_ENV if key in os.environ}
    settings, prefs = case_settings(case)
    env.update(settings)
    env.update(AURORA_UPDATE_TEST_ROOT=str(root), AURORA_CAPTURE=str(root / "frame.png"))
    return env, prefs


def validate_pixels(stats):
    if (stats["width"], stats["height"]) != (960, 560):
        raise RuntimeError(f"Unexpected capture dimensions: {stats}")
    if stats["interior_pixels"] <= 0:
        raise RuntimeError("Capture has no measurable interior")
    fraction = stats["interior_nonblack"] / stats["interior_pixels"]
    if fraction < 0.5 or stats["interior_rgb_bins"] < 16:
        raise RuntimeError(f"World capture is blank or nearly uniform: {stats}")


def gpu_errors(*logs):
    return re.findall(
        r"^.*(?:wgpu error|validation error|failed assertion|MTLCommandBufferErrorDomain|"
        r"GPU[ \t]+Timeout[ \t]+Error|kIOGPUCommandBufferCallbackErrorTimeout|SubmissionsIgnored|"
        r"command[ -]?buffer[^\n]*(?:abort|fail)|abort[^\n]*command[ -]?buffer|"
        r"GPU device lost|panicked at).*$",
        "\n".join(logs),
        flags=re.IGNORECASE | re.MULTILINE,
    )


def collect_native_log(pid, started, finished, output):
    # Query this process's lifetime only, after it has been reaped. A short
    # drain interval lets its final unified-log records reach the log store.
    # This reads existing messages; it enables no Metal instrumentation.
    command = [
        "/usr/bin/log", "show", "--style", "compact", "--info",
        "--start", started.strftime("%Y-%m-%d %H:%M:%S%z"),
        "--end", (finished + timedelta(seconds=1)).strftime("%Y-%m-%d %H:%M:%S%z"),
        "--predicate", f"processID == {pid}",
    ]
    report = {"pid": pid, "command": command, "exit": None, "passed": False}
    try:
        time.sleep(2)
        with (output / "native-system.log").open("wb") as stdout, (output / "native-system-stderr.log").open("wb") as stderr:
            result = subprocess.run(command, stdout=stdout, stderr=stderr, timeout=45)
        report["exit"] = result.returncode
        if result.returncode != 0:
            raise RuntimeError(f"Native log query exited with {result.returncode}; see native-system-stderr.log")
        native = (output / "native-system.log").read_text(errors="replace")
        errors = gpu_errors(native, (output / "native-system-stderr.log").read_text(errors="replace"))
        report["gpu_error_count"] = len(errors)
        report["gpu_errors"] = errors[:12]
        report["passed"] = not errors
    except (OSError, RuntimeError, subprocess.SubprocessError) as exc:
        report["failure"] = str(exc)
    finally:
        (output / "native-query.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def validate_log(log, console, native=""):
    if not re.search(r"GPU: .*\(Metal,", log):
        raise RuntimeError("The viewer did not confirm a Metal adapter")
    capture = re.search(r"\bframe 780 captured to [^\n]+", log)
    if capture is None:
        raise RuntimeError("The viewer did not confirm its frame 780 capture")
    states = re.findall(r"interface: UI rendering feature (off|on)\b", log[:capture.start()])
    if not states or states[-1] != "off":
        raise RuntimeError("UI was not disabled before the world capture")
    combined = "\n".join((log, console, native))
    if "using baked source" in combined or "offline water shader" in combined:
        raise RuntimeError("Unexpected offline shader override in the final build")
    errors = gpu_errors(combined)
    if errors:
        raise RuntimeError(f"GPU/validation failure ({len(errors)} matching lines):\n" + "\n".join(errors[:12]))


def run_case(viewer, inspector, output, case, identity):
    output.mkdir(parents=True, exist_ok=True)
    report = {**identity, "case": case, "mode": "normal", "passed": False, "exit": None}
    try:
        with tempfile.TemporaryDirectory(prefix=f"aurora-final-{case}-", dir=os.environ.get("RUNNER_TEMP")) as tmp:
            root = Path(tmp)
            install = root / "install"
            install.mkdir()
            exe = install / "aurora-viewer"
            shutil.copy2(viewer, exe)
            report["copied_executable_sha256"] = sha256(exe)
            if report["copied_executable_sha256"] != identity["executable_sha256"]:
                raise RuntimeError("Isolated viewer does not match the freshly built executable")
            marker = {
                "schema": 1,
                "feed": "http://127.0.0.1:1/feed.json",
                "public_key": "64b9f63e39ac482952f1ec5fb675b62b2331b272951e8292f5394dd84e099b7c",
            }
            (root / "aurora-update-test.json").write_text(json.dumps(marker))
            env, prefs = viewer_environment(case, root)
            (root / "profile").mkdir()
            (root / "profile/settings.json").write_text(json.dumps(prefs))
            settings = {key: value for key, value in env.items() if key not in INHERITED_ENV}
            settings.pop("AURORA_UPDATE_TEST_ROOT")
            settings.pop("AURORA_CAPTURE")
            (output / "settings.json").write_text(
                json.dumps({"mode": "normal", "environment": settings, "inherited_keys": sorted(set(env) & set(INHERITED_ENV)),
                            "profile": prefs}, indent=2) + "\n"
            )
            log = root / "logs/final-metal.log"
            frame = root / "frame.png"
            console_path = output / "console.log"
            print(f"Starting ordinary Metal case: {case}", flush=True)
            process = None
            started = datetime.now(timezone.utc)
            report["started_at"] = started.isoformat()
            try:
                with console_path.open("wb") as console:
                    process = subprocess.Popen(
                        [exe, "--title", f"Test final Metal {case}", "--language", "en"],
                        cwd=install, env=env, stdout=console, stderr=subprocess.STDOUT,
                    )
                    report["pid"] = process.pid
                    try:
                        process.communicate(timeout=600)
                        report["exit"] = process.returncode
                    except subprocess.TimeoutExpired:
                        report["exit"] = "timeout"
                    finally:
                        if process.poll() is None:
                            process.kill()
                            process.communicate(timeout=20)
                        report["reaped_returncode"] = process.returncode
            finally:
                finished = datetime.now(timezone.utc)
                report["finished_at"] = finished.isoformat()
                if log.exists():
                    shutil.copy2(log, output / "viewer.log")
                if frame.exists():
                    shutil.copy2(frame, output / "frame.png")
                if process is not None and process.poll() is not None:
                    report["native_log"] = collect_native_log(process.pid, started, finished, output)
            if report["exit"] == "timeout":
                raise RuntimeError("Viewer did not finish within ten minutes")
            if report["exit"] != 0:
                raise RuntimeError(f"Viewer exited with {report['exit']}")
            if not log.is_file() or not frame.is_file():
                raise RuntimeError("Viewer did not produce both its log and capture")
            # Retain the pixel table even if a visible image hid a native GPU
            # timeout. It cannot turn a failed command buffer into a pass.
            stats = json.loads(subprocess.check_output([inspector, output / "frame.png"], text=True, timeout=20))
            report["pixels"] = stats
            native_query = report["native_log"]
            if "failure" in native_query:
                raise RuntimeError("Native log collection failed: " + native_query["failure"])
            validate_log(
                log.read_text(errors="replace"), console_path.read_text(errors="replace"),
                (output / "native-system.log").read_text(errors="replace") + "\n"
                + (output / "native-system-stderr.log").read_text(errors="replace"),
            )
            validate_pixels(stats)
            report["passed"] = True
    except (OSError, RuntimeError, ValueError, KeyError, subprocess.SubprocessError) as exc:
        report["failure"] = str(exc)
    finally:
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2), flush=True)
    return report


def main():
    viewer, inspector, output = map(lambda s: Path(s).resolve(), sys.argv[1:4])
    output.mkdir(parents=True, exist_ok=True)
    report = {
        "source_commit": os.environ.get("SOURCE_COMMIT", ""),
        "source_tree": os.environ.get("SOURCE_TREE", ""),
        "mode": "normal",
        "passed": False,
        "cases": [],
    }
    try:
        for key in ("source_commit", "source_tree"):
            if re.fullmatch(r"[0-9a-f]{40}", report[key]) is None:
                raise RuntimeError(f"Invalid or missing exact source identity: {key}")
        report["executable_sha256"] = sha256(viewer)
        identity = {key: report[key] for key in ("source_commit", "source_tree", "executable_sha256")}
        for case in CASES:
            # Still collect the second independent case if the first fails.
            report["cases"].append(run_case(viewer, inspector, output / case, case, identity))
        failed = [result["case"] for result in report["cases"] if not result["passed"]]
        if failed:
            raise RuntimeError("Ordinary Metal capture failed: " + ", ".join(failed))
        report["passed"] = True
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as exc:
        report["failure"] = str(exc)
        raise
    finally:
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    main()
