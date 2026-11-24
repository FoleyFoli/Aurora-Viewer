$ErrorActionPreference = 'Stop'
$output = 'evidence/windows-smoke'
$result = Get-Content "$output/result.json" -Raw | ConvertFrom-Json
$log = Get-Content "$output/viewer.log" -Raw
if ($result.status -cne 'passed' -or $result.exit_code -ne 0 -or $result.diagnostic_mode -cne 'ordinary' -or
    $result.exe_sha256 -cne $env:EXE_SHA256 -or $result.software_driver.sha256 -cne $env:DRIVER_SHA256 -or
    $result.source_commit -cne $env:SOURCE_COMMIT -or $result.source_tree -cne $env:SOURCE_TREE -or
    $result.workflow_run -ne $env:GITHUB_RUN_ID -or $result.build_run_id -ne $env:BUILD_RUN_ID) {
    throw 'Unexpected native viewer, driver or source identity'
}
foreach ($message in @('new profile: graphics level High', 'demo soft shadows: 16 prims', 'interface: UI rendering feature off',
                       'sun shadows: SwiftShader uses the three-cascade layout; directional PCSS and contact shadows remain available',
                       'frame 650 captured to')) {
    if (!$log.Contains($message)) { throw "Missing native viewer marker $message" }
}
if ($result.shader_source -or $log.Contains('using baked source') -or $env:AURORA_DEMO_WATER_SHADERS) { throw 'Shader source override present' }
$expected = @{
    'reference/mesa/world-650.png' = 'e4894f9cc079ef2f97a96f05d80b9be109f7857fc1707371bc1ff4e290c70381'
    'reference/unshadowed/world.png' = 'aaa8014d884bf569cea537d0adbea8942b4c2e56f7e070f28209957fd341d74a'
}
foreach ($path in $expected.Keys) {
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected[$path]) {
        throw "Incorrect reference image $path"
    }
}
Add-Type -AssemblyName System.Drawing
$frame = [Drawing.Bitmap]::new((Resolve-Path "$output/world.png").Path)
$mesa = [Drawing.Bitmap]::new((Resolve-Path 'reference/mesa/world-650.png').Path)
$disabled = [Drawing.Bitmap]::new((Resolve-Path 'reference/unshadowed/world.png').Path)
try {
    if ($frame.Width -ne 800 -or $frame.Height -ne 560) { throw 'Unexpected dimensions' }
    $dark = 0; $darkPreserved = 0; $lit = 0; $litPreserved = 0
    $sumMeasured = 0.0; $sumReference = 0.0
    # Fixed rectangle on the static sloped receiver, clear of the animated avatar.
    for ($y=300; $y -lt 402; $y++) {
        for ($x=200; $x -lt 295; $x++) {
            $native = $frame.GetPixel($x, $y); $good = $mesa.GetPixel($x, $y); $bad = $disabled.GetPixel($x, $y)
            $negative = ([int]$bad.R + $bad.G + $bad.B) / 3.0
            $reference = $negative - ([int]$good.R + $good.G + $good.B) / 3.0
            $measured = $negative - ([int]$native.R + $native.G + $native.B) / 3.0
            if ($reference -ge 15.999) {
                $dark++; $sumReference += $reference; $sumMeasured += $measured
                if ($measured -ge 7.999) { $darkPreserved++ }
            } elseif ([Math]::Abs($reference) -le 2.001 -and $negative -gt 55.001) {
                $lit++
                if ([Math]::Abs($measured) -le 4.001) { $litPreserved++ }
            }
        }
    }
    $stats = [ordered]@{
        visible_shadow_reference = $expected['reference/mesa/world-650.png']
        disabled_shadow_reference = $expected['reference/unshadowed/world.png']
        capture_sha256 = (Get-FileHash -LiteralPath "$output/world.png" -Algorithm SHA256).Hash.ToLowerInvariant()
        core_pixels = $dark
        core_retained = $darkPreserved
        core_reference_darkening = $sumReference / $dark
        core_measured_darkening = $sumMeasured / $dark
        adjacent_lit_pixels = $lit
        adjacent_lit_retained = $litPreserved
    }
    $stats | ConvertTo-Json | Tee-Object -FilePath "$output/receiver.json"
    if ($dark -ne 6083 -or $lit -ne 3464 -or $darkPreserved -lt 0.80 * $dark -or $litPreserved -lt 0.80 * $lit) {
        throw 'The matched receiver lost too many shadowed or lit pixels'
    }
} finally { $frame.Dispose(); $mesa.Dispose(); $disabled.Dispose() }
