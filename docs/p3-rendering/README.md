# P3 rendering changes

This fork ports the P3 v2.4.3 renderer onto Aurora `v0.9.13-beta.20261121.2` (`231c88a3`). The directional maps, caster overlap, screen-space contact shadows, close-up crop and stock shadow tiers follow P3. Aurora's separate shadow intensity, photo controls and main-view projector filtering remain available. The branch includes Materials, Ambient Occlusion and Depth of Field.

[Complete change register and review notes](CHANGES.md) · [Validation and reproduction](VALIDATION.md)

## Shadow controls

In Graphics, choose a shadow quality level. Each enabled stock level uses graded directional maps, Bend screen-space contact shadows, and **Soft sun and moon shadows**. The last option gives a wider penumbra when a caster is farther from its receiving surface. **Light angular diameter** controls the apparent width of the emitter. The slightly wider Low and Medium defaults soften visible steps at those resolutions.

| Stock level | Map setting | Balanced sun shadow distance | Light angular diameter | Shadow-casting projector limit |
| --- | ---: | ---: | ---: | ---: |
| Low | 1024 | 48 m | 0.65° | 1 |
| Medium | 2048 | 96 m | 0.65° | 2 |
| High | 4096 | 144 m | 0.53° | 4 |
| Ultra | 4096 | 192 m | 0.53° | 8 |

The projector limit is a budget for simultaneous shadow casters. Available atlas space can reduce the effective count. **Custom** stores edits separately for each tier. Turning it off restores the tier’s stock values and retains edits for the next time Custom is enabled. Turning off **Soft sun and moon shadows** disables the angle control in place. Closing **Advanced** keeps its values active. The reset arrow beside Custom restores the selected tier. Named graphics profiles carry the shadow settings with them. [Tier recipes and persistence](../../crates/aurora-viewer/src/settings/shadow_profile.rs)

## How the directional maps and contact shadows interact

- **Different caster heights.** Layered soft shadows draw nearby and distant blockers into separate light-depth ranges. The remote layer uses tiles at half the local tile width and height. The filter samples both layers in corresponding directions across the sun disk and bounds the result by the local visibility. This addresses a moving overhead caster causing an already-shadowed point to brighten. Casters falling within one depth range still share a blocker estimate.
- **Very close receivers.** An optional camera-centred 1024 × 1024 depth map covers a 50 cm square perpendicular to the light. Its 0.49 mm texels can better align a thin rim’s directional shadow with its screen-space contact shadow. The crop snaps to the parent light-space grid and retains its depth projection. The shader still looks up distant blockers in the larger map. The effect fades across the outer 10 cm of the crop and over 32–45 cm of camera-to-surface distance. Optical zoom alone leaves that distance unchanged.
- **Composition and cost.** The darker of directional and effective contact visibility determines direct-light visibility. The extra distant mip adds 25% of the base atlas allocation when active and replays directional caster draws. The close map adds a 4 MiB depth allocation and one caster pass on eligible frames. It requires a graded layout, useful finer coverage and adequate device limits. The renderer can use the ordinary detail map when those conditions fail. These figures describe allocations and passes. Desktop GPU timings remain to be measured. [Directional lookup](../../crates/aurora-render/src/shaders/common.wgsl) · [Projection and map allocation](../../crates/aurora-render/src/renderer.rs)

## Other rendering modules

| Module | Controls and defaults | Useful comparison |
| --- | --- | --- |
| Materials | Thin-surface PBR lighting is off initially. Select a material UUID or explicitly preview all double-sided, alpha-masked PBR surfaces. Controls adjust the authored AO floor and approximate backlighting. A separate legacy-lighting switch, also initially off, adjusts legacy diffuse response and duplicate local highlights. | Highlight the matched foliage and compare its front and back faces with an adjacent untargeted surface. The experiment affects local rendering only. |
| Ambient occlusion | **Thin-object AO compensation** adjusts GTAO for narrow occluders. Zero preserves the stock response. | Increase the value gradually on leaves or hair and inspect a narrow solid crevice for loss of contact darkening. |
| Depth of field | Blended-object focus depth, smoother near-focus blur and the focus-zone overlay each start off. The depth option targets alpha-blended surfaces with alpha of at least 0.5. | Compare foreground blur on a mostly opaque blended surface. Fine transparent hair and multiple overlapping translucent layers remain difficult. |

[Graphics controls](../../crates/aurora-viewer/src/ui/options/pages/graphics.rs) · [Validation and reproduction](VALIDATION.md)
