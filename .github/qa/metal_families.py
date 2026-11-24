"""Isolate directional-shadow code without changing the executable or host passes."""
import hashlib
import re

SOURCE_HASH = "33a3070f6e044399697172750afb19625dcfe0637cb301fcabc09c22d7517667"
HASHES = {
    "directional-debug-calls-off": "f7379fe0b4b6907644d123a0ad309e0694cd6b5e8b20ca3bb029dc109eee8b77",
    "directional-local-hard": "a02060a16d7381026a6313820c465b986fd62f7ac35131fb8a025560d7367817",
    "directional-remote-hard": "ae316baea30c3db27281cddc887b3758e292dea6c35683461d06b2c74df4a3d6",
    "directional-both-hard": "61514d0ab2a5dca983e73426716449a9a5c2bd688e7c737a9dc85e6edabce992",
}


def function_body(source, name):
    match = re.search(r"^fn " + name + r"\([^{}]+?\)\s*->\s*[^{}]+?\{", source, flags=re.M)
    assert match, name
    start = match.end()
    depth = 1
    for index in range(start, len(source)):
        char = source[index]
        depth += (char == "{") - (char == "}")
        if depth == 0:
            return start, index
    raise ValueError(name)


def replace_body(source, name, transformation):
    first, last = function_body(source, name)
    return source[:first] + transformation(source[first:last]) + source[last:]


def diagnostic_calls(body):
    # Only user-selected measurement views change. Preserve every production-material
    # callsite, all external shaders and all other diagnostic modes.
    for text in ["return vec3<f32>(shadow_factor(world_pos, n, vertex_depth));",
                 "return vec3<f32>(sun_visibility(world_pos, n, vertex_depth, frag));"]:
        assert body.count(text) == 1
        body = body.replace(text, "return col;")
    marker = "    if (view != RC_MODE_CASCADES) { return col; }"
    assert body.count(marker) == 1
    return body.split(marker)[0] + "    return col;\n"


def hard_local(body):
    assert "return vec3<f32>(sum / (1.0 + middle + wide), blend, radius);" in body
    # The normal fallback for zero local blend is still evaluated by every caller.
    return "\n    return vec3<f32>(0.0);\n"


def hard_remote(body):
    assert body.count("    var radius = 0.0;") == 1
    result = "return min(local_visibility, distant_visibility(remote, remote.position, ref_z, 0.0));"
    assert body.count(result) == 1
    # Keep the original paired-slab hard-shadow response (bilinear mip-1 depth).
    return body.split("    var radius = 0.0;")[0] + "    " + result + "\n"


def family_override(source, mode):
    if hashlib.sha256(source.encode()).hexdigest() != SOURCE_HASH:
        raise ValueError("Unrecognized archived shader")
    if mode == "directional-debug-calls-off":
        source = replace_body(source, "debug_view", diagnostic_calls)
    if mode in {"directional-local-hard", "directional-both-hard"}:
        source = replace_body(source, "directional_pcss", hard_local)
    if mode in {"directional-remote-hard", "directional-both-hard"}:
        source = replace_body(source, "pcss_compose_grids", hard_remote)
    if hashlib.sha256(source.encode()).hexdigest() != HASHES[mode]:
        raise ValueError("Directional diagnostic does not match the validated shader")
    return source
