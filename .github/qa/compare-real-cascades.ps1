$ErrorActionPreference = 'Stop'
$output = 'evidence/windows-smoke'
$negativeHash = (Get-FileHash 'reference/unshadowed/world.png' -Algorithm SHA256).Hash.ToLowerInvariant()
$chromeHash = (Get-FileHash 'reference/chrome/world.png' -Algorithm SHA256).Hash.ToLowerInvariant()
if ($negativeHash -cne 'aaa8014d884bf569cea537d0adbea8942b4c2e56f7e070f28209957fd341d74a' -or
    $chromeHash -cne '7f7735c1112d17571ac19138b5f30181aa9253ae7df6b234d10d25874ed4fa91') { throw 'Incorrect previous capture identity' }
Add-Type -AssemblyName System.Drawing
$frame = [Drawing.Bitmap]::new((Resolve-Path 'reference/chrome/world.png').Path)
$mesa = [Drawing.Bitmap]::new((Resolve-Path "$output/world-650.png").Path)
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
        visible_shadow_reference = (Get-FileHash -LiteralPath "$output/world-650.png" -Algorithm SHA256).Hash.ToLowerInvariant()
        disabled_shadow_reference = $negativeHash
        capture_sha256 = $chromeHash
        core_pixels = $dark
        core_retained = $darkPreserved
        core_reference_darkening = $sumReference / $dark
        core_measured_darkening = $sumMeasured / $dark
        adjacent_lit_pixels = $lit
        adjacent_lit_retained = $litPreserved
    }
    $stats | ConvertTo-Json | Tee-Object -FilePath "$output/receiver.json"
    if ($dark -lt 1000 -or $lit -lt 1000 -or $darkPreserved -lt 0.80 * $dark -or $litPreserved -lt 0.80 * $lit) {
        throw 'The matched receiver lost too many shadowed or lit pixels'
    }
} finally { $frame.Dispose(); $mesa.Dispose(); $disabled.Dispose() }
