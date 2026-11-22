# Validating P3 v2.4.3

## Checks on the beta port

A headless SwiftShader Vulkan run sampled the ported lookup and contact shaders in five close-range scenes. The directional visibility matched the v2.4.3 readbacks at all 184,168 sampled visible points. A single rim on Low produced zero false double bands in 285 scans, with a mean absolute visibility error of 2.07 percentage points. A plate 12 m away crossed the rim without brightening the local shadow. The empty receiver and two physical rims were also checked. A separate ground fixture admitted a 500 m caster and measured zero brightening under the local caster. These are synthetic, fixed-geometry comparisons to the P3 reference. They do not measure desktop frame time or establish a result on a particular avatar.

## Original v2.4.3 reference

The v2.4.3 release kit contains software-Vulkan captures of the production directional lookup and contact shaders. The close-up fixture places a 1.8 mm opaque rim 9 mm above a curved receiver. A reference traces 256 directions across the sun disk. At Low, the camera sat 16 cm from the surface and moved in 1.6 mm lateral steps. Five views contributed 285 receiver scans. Measurements compare the two rendering checkpoints on the same synthetic geometry.

| Low, single rim | v2.4.2 | v2.4.3 |
| --- | ---: | ---: |
| Median directional shadow centre error | 1.12 mm | 0.08 mm |
| Median excess combined-shadow width | 3.15 mm | 1.05 mm |
| Scans with a false second band | 6 / 285 | 0 / 285 |
| 95th percentile change in direct visibility between frames | 7.17 percentage points | 0.39 percentage points |

On the slow pullback the two lookups agreed beyond 45 cm. The faster pullback had a larger 95th-percentile frame change, from 11.37 to 12.94 percentage points. At a cropped offscreen edge, the newer version had six false contours in 116 scans where the earlier version had zero. Two real rims retained more light in the gap between them. A 12 m plate crossing a single local rim caused zero measured local brightening in that fixture. The separate 500 m control depends on whether the configured light-depth volume admits the caster. A caster outside the volume cannot test overlap.

The archived logs record a locked `cargo check` of the viewer with all modules and with Shadows alone, 30 shader layout permutations, 240 fragment specializations, and agreement for all 94 Rust/WGSL frame uniform offsets. The profile harness records 32 passing cases. Three viewer tests covered legacy angles and layout/input stability of the Custom controls. The recorded Windows builder workflow used PowerShell on Linux with a stand-in compiler for build and launch. The checkpoint evidence leaves native Windows Release compilation, desktop GPU frame time and repeatable avatar captures open.

## Reproduce the evidence

The original v2.4.3 validation archive is `Aurora-Contact-Hardening-v2.4.3.zip`. Its `VALIDATION.md` documents the sampling definitions and its `validation/` directory contains readbacks, plots, logs and replay instructions. From the **root of that extracted archive**, with a shell that has Python 3.10 or later, substitute the original v2.4.3 Git checkout path in the commands below. The saved-data audit requires NumPy. Re-rendering the scene also requires the fixture’s Rust dependencies and a Vulkan adapter.

```sh
python tools/verify.py --source /path/to/fork --checkpoint /path/to/fork
python validation/close-rim/audit.py validation/close-rim/captures --out close-rim-results.json
```

The first command checks every optional module combination against the base Git objects and compares the integrated source files to the release fingerprints. The second recalculates visibility and motion metrics from the saved GPU data. For a fresh GPU run, follow `validation/close-rim/README.md` inside the archive. The fork’s normal repository checks are documented in [AGENTS.md](../../AGENTS.md) and [the upstream testing guide](../TESTING.md).

## In-viewer test matrix

Hold the light direction, camera path, graphics profile and viewport size constant between builds. Record the commit, GPU and driver, internal resolution or upscaler, shadow tier and Custom overrides. Use a short clip for visibility changes during motion, and the same camera pose for diagnostic screenshots.

| Scene | Procedure and expected result |
| --- | --- |
| Glasses rim on a face | Hold the avatar still. Move the camera sideways by a few millimetres, then pull back through 32–45 cm. A single rim should leave one band on the skin. Two physical rims should retain a lit gap. If a band splits, keep contact shadows on and compare **Shadow-map visibility**, **Effective contact visibility** and **Combined shadow visibility** under Custom → Advanced → Shadow diagnostics. |
| Low and Medium edges | Slowly move toward and away from an oblique caster over flat terrain. Check the dark side of the silhouette as it crosses cascade boundaries. Repeat on High at the same poses. Record any repeating steps, holes under feet or detached edges. |
| Moving overhead shadow | Hold the camera and a local tree or avatar still. Move a large caster across its shadow. First isolate the distant caster to confirm its shadow reaches the receiver. As it crosses the local shadow, surrounding ground can darken and reduce contrast. Look for the already-dark core getting **brighter** or briefly disappearing. |
| Profiles and projectors | Edit the High Custom angle, turn soft shadows off and on, switch tiers, and return. Confirm the angle and row positions. Save and reload a named profile. Reset High and check Medium retains its edits. With several active projectors, check the budget at each tier. |
| Materials, AO and focus | Toggle one module control at a time using the comparison scenes in the [feature guide](README.md). For DoF, check the focus zone with blended surfaces, then repeat in Photo mode. For GTAO and material matching, include a neighbouring opaque object as a control. |
| Frame time | Compare the same view at the same internal resolution after shader warm-up. Include a static close-up, a moving caster and a dense scene on an entry-level adapter. Record GPU frame time in milliseconds, the statistic and sample duration, and whether the close-map path is available. |
