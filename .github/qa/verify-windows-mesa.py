"""Independently verify native Windows render evidence from its original Actions artifact."""
import hashlib
import io
import json
import re
import sys
from pathlib import Path
from zipfile import ZipFile

from PIL import Image

TARGETS = {
    "repaired": {
        "source_commit": "75c022abdbaad690ab14975674ec900ace2c51fe",
        "source_tree": "4d449b0369533d5e5ea857b09da08b6040c2788e",
        "zip_sha256": "0690bf78bdf0d37168af686c8de4f580c4cdc66848414f8595e7e134eca37e05",
        "exe_sha256": "efb10d920947afeed6f424df6675cf70aea98d228adff14ad4db375b6ea864ce",
        "build_workflow_commit": "e5b5daf750c940b4a23e3ef6c96fbef5183de01c",
        "build_run_id": "45346318145",
        "package_artifact_id": 14903570183,
        "package_artifact_sha256": "c6dd003a9b2165b3b2520fdf538d07dbb5a0cb1091510c3bcdb9b8746f52892a",
        "workflow_commit": "b07e51359e6199ce9bc8a8d5cff6b19e512d8af2",
        "workflow_run": "45360112796",
        "job_id": 138327723503,
        "frames": {650: "world.png"}
    },
    "final": {
        "source_commit": "a9ba60f9c985eba6f80df68fcf40212c14cae2d2",
        "source_tree": "d8588e7fc57af407be6c340cf9ad52b5a15ed220",
        "zip_sha256": "95797c63f956987f49588473e8a4e4c5f0db38dc1a0e53dc5266b0b097dc97ba",
        "exe_sha256": "0d9e997c09235e3529e2e2a7aee34ecd71f8077fbcf20ea03e49ed747cd8e848",
        "build_workflow_commit": "76f2f7667b686306bda40e730e6861d8dcb35d12",
        "build_run_id": "45359333007",
        "package_artifact_id": 14906260621,
        "package_artifact_sha256": "1ffdcb00baa1094b31369d00a3bbaf3e7374b50fef243460a5e1dfa4bb11e696",
        "workflow_commit": "0c1cec44d37a038e20a3983aeba9f26b752de47e",
        "workflow_run": "45363769384",
        "job_id": 138340116333,
        "frames": {60: "world-60.png", 605: "world-605.png", 650: "world-650.png"}
    }
}


def require(value, message):
    if not value:
        raise ValueError(message)


def measure(raw):
    with Image.open(io.BytesIO(raw)) as image:
        require(image.format == "PNG" and image.size == (800, 560), "Expected an 800x560 PNG")
        rgb = image.convert("RGB")
        colors = set()
        nonblack = 0
        samples = 0
        for y in range(112, 448, 4):
            for x in range(160, 640, 4):
                pixel = rgb.getpixel((x, y))
                nonblack += max(pixel) > 8
                colors.add(tuple(channel >> 4 for channel in pixel))
                samples += 1
    return {"size": [800, 560], "samples": samples, "nonblack": nonblack,
            "rgb_444_bins": len(colors), "sha256": hashlib.sha256(raw).hexdigest()}


def verify_render(read, expected):
    result = json.loads(read("result.json"))
    for key in ("source_commit", "source_tree", "build_workflow_commit", "build_run_id",
                "workflow_commit", "workflow_run", "zip_sha256", "exe_sha256"):
        require(result[key] == expected[key], f"Run metadata mismatch in {key}")
    package = json.loads(read("source-artifact.json"))
    require(package["id"] == expected["package_artifact_id"] and
            package["digest"] == "sha256:" + expected["package_artifact_sha256"] and
            package["name"] == "Aurora-Viewer-rendering-test-" + expected["source_commit"] and
            str(package["workflow_run"]["id"]) == expected["build_run_id"] and
            package["workflow_run"]["head_sha"] == expected["build_workflow_commit"],
            "Package artifact provenance mismatch")
    require(result["process_started"] is True and result["exit_code"] == 0, "Viewer did not exit cleanly")
    require(result["status"] in ("passed", "failed"), "Unrecognised raw result")
    if result["status"] == "failed":
        require(re.fullmatch(r"""Exception calling "ReadAllText" with "1" argument\(s\): "The process cannot access the file '[^'\r\n]+\\std(?:err|out)\.txt' because it is being used by another process\." """.strip(),
                             result.get("detail", "")), "Unexpected original smoke failure: " + result.get("detail", ""))
    driver = result["software_driver"]
    require(driver["dll_sha256"] == "ca4814fc85a75179af05ad00092e4b09c583e8fede67f1c0391e366019972f3b",
            "Unexpected Mesa DLL")
    require("mingw-w64-ucrt-x86_64-mesa 26.3.1-1" in read("driver-packages.txt").decode(),
            "Signed Mesa package was not recorded")
    require("llvmpipe" in read("vulkaninfo.txt").decode(), "Software adapter preflight missing")
    log = read("viewer.log").decode()
    streams = [log, read("stderr.txt").decode(), read("stdout.txt").decode()]
    fatal = re.compile(r"wgpu error:|validation error|GPU device lost|panicked at", re.I)
    require(all(not fatal.search(stream) for stream in streams), "Found a GPU or viewer failure")
    adapter = result["adapter"]
    require("llvmpipe Mesa 26.3.1" in adapter and "GPU: " + adapter in log, "Wrong active Vulkan adapter")
    frames = {}
    for number, name in expected["frames"].items():
        capture = re.search(rf"\bframe {number} captured to [^\n]*[\\/]{re.escape(name)}\s*$", log, re.M)
        require(capture is not None, f"Capture {name} absent from viewer log")
        before = log[:capture.start()]
        require("demo soft shadows: 16 prims" in before and
                "world resolution: 800x560 (output 800x560, scale 100%)" in before,
                "Expected slope fixture and world resolution")
        metrics = measure(read(name))
        ui_states = re.findall(r"interface: UI rendering feature (off|on)\b", before)
        metrics["ui_off_before_capture"] = bool(ui_states and ui_states[-1] == "off")
        metrics["file"] = name
        if number == 650:
            require(metrics["ui_off_before_capture"] and
                    "startup window shown after the first presented frame" in before and
                    "contact shadows: cached 120 samples per pixel" in before,
                    "Missing world presentation, UI-off or contact-shadow evidence")
            require(metrics["nonblack"] >= metrics["samples"] * 0.5 and metrics["rgb_444_bins"] >= 16,
                    "Blank or near-uniform world interior")
        frames[str(number)] = metrics
    return {"source_commit": expected["source_commit"], "source_tree": expected["source_tree"],
            "build_run_id": expected["build_run_id"], "package_artifact_id": package["id"],
            "package_artifact_digest": package["digest"], "portable_zip_sha256": result["zip_sha256"],
            "exe_sha256": result["exe_sha256"], "renderer_run_id": result["workflow_run"],
            "renderer_job_id": expected["job_id"], "renderer_report_status": result["status"],
            "renderer_report_detail": result.get("detail"), "exit_code": result["exit_code"],
            "gpu_or_viewer_fatal_matches": 0, "adapter": adapter,
            "scene": "offline slope, 16 prims", "world_size": [800, 560], "scale_percent": 100,
            "requested": {"shadow_level": 3, "soft_shadows": 1, "contact_shadows": 1, "rt_shadows": 0},
            "mesa_driver_sha256": driver["dll_sha256"], "frames": frames}


def verify_archive(key, directory):
    expected = TARGETS[key]
    run = json.loads((directory / "run.json").read_bytes())
    job = json.loads((directory / "job.json").read_bytes())
    require(run["status"] == "completed" and run["event"] == "push" and
            run["head_branch"] == "validation/windows-shadow-lavapipe" and
            run["path"] == ".github/workflows/rendering-windows-lavapipe.yml" and
            run["head_sha"] == expected["workflow_commit"] and
            run["repository"]["full_name"] == run["head_repository"]["full_name"] == "FoleyFoli/Aurora-Viewer" and
            str(run["id"]) == expected["workflow_run"], "Wrong original workflow run")
    require(job["id"] == expected["job_id"] and job["run_id"] == run["id"] and
            job["head_sha"] == expected["workflow_commit"] and job["conclusion"] in ("success", "failure") and
            any("windows" in label.lower() for label in job["labels"]) and
            job["name"] == "Launch the packaged viewer with MSYS2 Mesa lavapipe (software Vulkan)",
            "Wrong original Windows renderer job")
    artifacts = json.loads((directory / "artifacts.json").read_bytes())["artifacts"]
    require(len(artifacts) == 1, "Expected exactly one original evidence archive")
    artifact, = artifacts
    require(artifact["name"] == "rendering-final-windows-lavapipe-" + expected["source_commit"] and
            artifact["workflow_run"]["id"] == run["id"] and
            artifact["workflow_run"]["head_sha"] == expected["workflow_commit"] and not artifact["expired"],
            "Unrelated or expired raw render artifact")
    payload = (directory / "render.zip").read_bytes()
    digest = "sha256:" + hashlib.sha256(payload).hexdigest()
    require(digest == artifact["digest"] and len(payload) == artifact["size_in_bytes"], "Raw archive integrity mismatch")
    with ZipFile(io.BytesIO(payload)) as raw:
        require(len(raw.namelist()) == len(set(raw.namelist())), "Duplicate raw evidence paths")
        proof = verify_render(raw.read, expected)
    require((job["conclusion"] == "success") == (proof["renderer_report_status"] == "passed"),
            "Original Actions conclusion disagrees with result.json")
    failed_steps = [step["name"] for step in job["steps"] if step["conclusion"] == "failure"]
    if job["conclusion"] == "failure":
        require(failed_steps == ["Verify and launch the downloaded EXE and inspect its world capture"],
                "An additional original Windows job step failed")
    proof.update(render_artifact_id=artifact["id"], render_artifact_digest=digest,
                 original_job_conclusion=job["conclusion"], original_failed_steps=failed_steps)
    return proof


if __name__ == "__main__":
    selected = sys.argv[1]
    proof = verify_archive(selected, Path(sys.argv[2]))
    encoded = json.dumps(proof, indent=2) + "\n"
    Path(f"windows-{selected}-verification.json").write_text(encoded)
    print(encoded, end="")
