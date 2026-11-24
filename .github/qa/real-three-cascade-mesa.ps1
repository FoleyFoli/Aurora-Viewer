# Fork-only job. Exercise the downloaded portable ZIP on a fresh Windows runner.
$ErrorActionPreference = 'Stop'
$output = Join-Path $env:GITHUB_WORKSPACE 'evidence/windows-smoke'
$root = Join-Path $env:RUNNER_TEMP "aurora-windows-smoke-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"
$install = Join-Path $root 'install'
$log = Join-Path $root 'logs/windows-smoke.log'
$frame = Join-Path $output 'world.png'
$name = "Aurora-Viewer-rendering-test-$env:SOURCE_COMMIT"
$buildWorkflowCommit = if ($env:BUILD_WORKFLOW_COMMIT) { $env:BUILD_WORKFLOW_COMMIT } else { $env:GITHUB_SHA }
$buildRunId = if ($env:BUILD_RUN_ID) { $env:BUILD_RUN_ID } else { $env:GITHUB_RUN_ID }
$result = [ordered]@{
    status = 'failed'
    requested_driver = 'MSYS2 Mesa lavapipe (software Vulkan)'
    source_commit = $env:SOURCE_COMMIT
    source_tree = $env:SOURCE_TREE
    build_workflow_commit = $buildWorkflowCommit
    build_run_id = $buildRunId
    workflow_commit = $env:GITHUB_SHA
    workflow_run = $env:GITHUB_RUN_ID
    process_started = $false
    adapter = $null
}
New-Item -ItemType Directory -Path $output -Force | Out-Null
Add-Type -AssemblyName System.Drawing
function Measure-Frame([string]$path) {
    $image = [Drawing.Bitmap]::new($path)
    try {
        $colors = [Collections.Generic.HashSet[int]]::new()
        $light = 0
        $count = 0
        for ($y = [int]($image.Height * 0.2); $y -lt $image.Height * 0.8; $y += 4) {
            for ($x = [int]($image.Width * 0.2); $x -lt $image.Width * 0.8; $x += 4) {
                $c = $image.GetPixel($x, $y)
                if ([Math]::Max($c.R,[Math]::Max($c.G,$c.B)) -gt 8) { $light++ }
                $bin = (([int]$c.R -shr 4) -shl 8) -bor (([int]$c.G -shr 4) -shl 4) -bor ([int]$c.B -shr 4)
                [void]$colors.Add($bin)
                $count++
            }
        }
        return @{ width = $image.Width; height = $image.Height; samples = $count; nonblack = $light; rgb_444_bins = $colors.Count
            visual_nonuniform = $image.Width -ge 400 -and $image.Height -ge 280 -and $light -ge $count * 0.5 -and $colors.Count -ge 16 }
    } finally { $image.Dispose() }
}
$proc = $null
try {
    $zip = Join-Path $env:GITHUB_WORKSPACE "test-client/$name.zip"
    $sum = (Get-Content -LiteralPath "$zip.sha256" -Raw).Trim()
    $result.zip_sha256 = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sum -cne "$($result.zip_sha256)  $name.zip") { throw 'Portable ZIP checksum mismatch' }
    New-Item -ItemType Directory -Path $root | Out-Null
    Expand-Archive -LiteralPath $zip -DestinationPath $root
    Rename-Item -LiteralPath (Join-Path $root $name) -NewName 'install'
    $manifest = Get-Content -LiteralPath (Join-Path $install 'build-manifest.json') -Raw | ConvertFrom-Json
    $exe = Join-Path $install 'aurora-viewer.exe'
    $result.exe_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($manifest.source_commit -cne $env:SOURCE_COMMIT -or $manifest.source_tree -cne $env:SOURCE_TREE -or
        $manifest.workflow_commit -cne $buildWorkflowCommit -or
        $manifest.workflow_run -cne "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$buildRunId" -or
        $manifest.executable.sha256 -cne $result.exe_sha256) {
        throw 'Packaged executable or source identity differs from the build manifest'
    }
    if ($env:WINDOWS_SMOKE_DRIVER -ne 'mesa-lavapipe') { throw 'Explicit software-driver opt-in is required' }
    $bin = Join-Path $env:MESA_UCRT64 'bin'
    $driver = Join-Path $bin 'vulkan_lvp.dll'
    $loader = Join-Path $bin 'vulkan-1.dll'
    $manifestPath = Join-Path $env:MESA_UCRT64 'share/vulkan/icd.d/lvp_icd.x86_64.json'
    $env:PATH = "$bin;$env:PATH"
    $driverHash = (Get-FileHash -LiteralPath $driver -Algorithm SHA256).Hash.ToLowerInvariant()
    # Bytes from the official MSYS2 Mesa 26.3.1-1 signed package.
    if ($driverHash -cne 'ca4814fc85a75179af05ad00092e4b09c583e8fede67f1c0391e366019972f3b') { throw 'Unexpected lavapipe DLL identity' }
    $target = Join-Path $install 'vulkan-1.dll'
    if (Test-Path -LiteralPath $target) { throw 'Packaged archive already includes a Vulkan loader' }
    Copy-Item -LiteralPath $loader -Destination $target
    $env:VK_DRIVER_FILES = $manifestPath
    $env:AURORA_GPU = 'llvmpipe'
    $result.software_driver = @{
        driver = $driver
        dll_sha256 = $driverHash
        loader_sha256 = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
        manifest = $manifestPath
        discovery = 'process environment'
    }
    $vulkanInfo = Join-Path $env:RUNNER_TEMP 'aurora-vk-probe/probe.exe'
    $deviceReport = & $vulkanInfo 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0 -or $deviceReport -notmatch 'llvmpipe|lavapipe') {
        # Elevated Windows processes can ignore a per-process driver manifest.
        # Register this package's signed driver only on the disposable runner.
        $reg = 'HKLM:\SOFTWARE\Khronos\Vulkan\Drivers'
        if (!(Test-Path -LiteralPath $reg)) { New-Item -Path $reg -Force | Out-Null }
        $old = Get-ItemProperty -LiteralPath $reg
        if ($old.PSObject.Properties[$manifestPath]) { throw 'Unanticipated existing lavapipe driver registration' }
        New-ItemProperty -LiteralPath $reg -Name $manifestPath -Value 0 -PropertyType DWord | Out-Null
        $registered = $true
        $result.software_driver.discovery = 'temporary runner registration'
        $deviceReport = & $vulkanInfo 2>&1 | Out-String
    }
    $deviceReport | Set-Content -LiteralPath (Join-Path $output 'vulkaninfo.txt') -Encoding utf8NoBOM
    if ($LASTEXITCODE -ne 0 -or $deviceReport -notmatch 'llvmpipe|lavapipe') { throw 'Vulkan software adapter preflight failed' }
    # The updater requires the executable to be inside <root>/install before
    # accepting a test profile, an isolated credential store and loopback-only feed.
    @{
        schema = 1
        feed = 'http://127.0.0.1:1/feed.json'
        public_key = '64b9f63e39ac482952f1ec5fb675b62b2331b272951e8292f5394dd84e099b7c'
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'aurora-update-test.json') -Encoding utf8NoBOM
    $env:AURORA_UPDATE_TEST_ROOT = $root
    $env:AURORA_DEMO = '1'
    $env:AURORA_WINDOW_SIZE = '800,560'
    $env:AURORA_LOG_NAME = 'windows-smoke'
    $env:AURORA_DEMO_SOFT_SHADOWS = 'slope'
    $env:AURORA_DEMO_POS = '132,126'
    $env:AURORA_DEMO_SUN = '27,45'
    $env:AURORA_SHADOWS = '3'
    $env:AURORA_SOFT_SHADOWS = '1'
    $env:AURORA_CONTACT_SHADOWS = '1'
    $env:AURORA_RT_SHADOWS = '0'
    $env:AURORA_DEMO_UI_FEATURE = 'off'
    $env:AURORA_CAPTURE = $frame
    $env:AURORA_CAPTURE_FRAMES = '60,605,650'
    $env:AURORA_CAPTURE_EXIT = '1'
    $env:RUST_LOG = 'info'
    # Obtain the native viewer's selected High settings with the same fresh profile and fixture.
    # The first capture only seeds settings; it is deliberately outside the measured evidence.
    $env:AURORA_CAPTURE = Join-Path $root 'seed.png'
    $env:AURORA_CAPTURE_FRAMES = '2'
    $env:AURORA_LOG_NAME = 'seed-high'
    $proc = Start-Process -FilePath $exe -ArgumentList '--title','Seed-High','--language','en' -WorkingDirectory $install -PassThru
    if (!$proc.WaitForExit(600000) -or $proc.ExitCode -ne 0) { throw 'Mesa failed to produce a clean fresh High profile' }
    $seedLog = Get-Content (Join-Path $root 'logs/seed-high.log') -Raw
    if (!$seedLog.Contains('new profile: graphics level High') -or !$seedLog.Contains('demo soft shadows: 16 prims')) { throw 'Missing native High settings' }
    $settingsPath = Join-Path $root 'profile/settings.json'
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json -AsHashtable
    function Record-Settings($settings, $path) {
        $keys = $settings.Keys | Where-Object { $_ -match 'shadow|draw_distance|^aa$|camera_fov' }
        $selected = [ordered]@{}
        foreach ($key in $keys) { $selected[$key] = $settings[$key] }
        $selected | ConvertTo-Json -Depth 100 | Set-Content $path -Encoding utf8NoBOM
    }
    Record-Settings $settings (Join-Path $output 'fresh-high-shadow-settings.json')
    if ($settings.graded_sun_shadows -ne $true -or $settings.shadow_quality -ne 3 -or
        $settings.shadow_map_resolution -notin @(0,2048) -or $settings.graded_shadow_distance -lt 16 -or
        $settings.graded_shadow_distance -gt 512 -or !$settings.sun_shadow_contact_hardening) { throw 'Native High shadow profile differs from the requested starting state' }
    $reach = $settings.graded_shadow_distance
    # From the exact app's Settings::shadow_params: quality 3 graded resolves to 2048 px.
    # Copy the graded reach into the conventional cascade reach so allocation alone changes.
    $settings.graded_sun_shadows = $false
    $settings.shadow_map_resolution = 2048
    $settings.three_level_shadow_distance = $reach
    # Import the flat active values verbatim instead of reapplying a saved graded custom slot.
    $settings.shadow_profile = $null
    $settings | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
    Record-Settings $settings (Join-Path $output 'matched-input-shadow-settings.json')
    # The diagnostic tier override clears explicit pixel sizes. Its prior value is now in settings.
    Remove-Item Env:AURORA_SHADOWS
    $env:AURORA_LOG_NAME = 'windows-smoke'
    $env:AURORA_CAPTURE = $frame
    $env:AURORA_CAPTURE_FRAMES = '60,605,650'
    $started = Get-Date
    $lastLogSize = 0
    $proc = Start-Process -FilePath $exe -ArgumentList '--title','Test-Windows-Shading','--language','en' -WorkingDirectory $install -PassThru -RedirectStandardOutput (Join-Path $output 'stdout.txt') -RedirectStandardError (Join-Path $output 'stderr.txt')
    $result.process_started = $true
    $result.pid = $proc.Id
    $noAdapter = 'Vulkan lists no graphics card after all retries|no native GPU adapter can present to the window'
    $limit = [DateTime]::UtcNow.AddMinutes(60)
    do {
        $exited = $proc.WaitForExit(3000)
        # Missing Vulkan often opens a native startup-error dialog indefinitely.
        if (Test-Path -LiteralPath $log) {
            # The logger keeps its write handle open between flushes.
            $stream = [IO.File]::Open($log, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
            try {
                $reader = [IO.StreamReader]::new($stream)
                try { $runtimeLog = $reader.ReadToEnd() } finally { $reader.Dispose() }
            } finally { $stream.Dispose() }
            if ($runtimeLog.Length -ne $lastLogSize) {
                $newLines = $runtimeLog.Substring([Math]::Min($lastLogSize, $runtimeLog.Length)) -split "`n"
                $newLines | Where-Object { $_ -match 'GPU: |renderer ready:|demo soft shadows:|world resolution:|first presented frame|UI rendering feature|captured to |wgpu error:|GPU device lost|the graphics card finished its frame again' } | ForEach-Object { Write-Host $_ }
                $lastLogSize = $runtimeLog.Length
            }
            if ($runtimeLog -notmatch 'GPU: [^\r\n]+\(Vulkan, driver ' -and $runtimeLog -match $noAdapter) {
                $result.status = 'unavailable-no-presentable-vulkan-adapter'
                throw $Matches[0]
            }
        }
        if (!$exited -and [DateTime]::UtcNow -ge $limit) { throw 'Timed out waiting for a native world capture' }
    } while (!$exited)
    $result.exit_code = $proc.ExitCode
    if ($proc.ExitCode -ne 0) { throw "Packaged viewer exited with $($proc.ExitCode)" }
    $runtimeLog = [IO.File]::ReadAllText($log)
    if ($runtimeLog -notmatch 'GPU: ([^\r\n]+\(Vulkan, driver [^\r\n]+)') { throw 'Viewer did not confirm its Vulkan adapter' }
    $result.adapter = $Matches[1].Trim()
    if ($result.adapter -notmatch 'llvmpipe|lavapipe') { throw 'Viewer did not confirm the requested software Vulkan adapter' }
    $result.adapter_candidates = @([regex]::Matches($runtimeLog, 'GPU adapter \d+: [^\r\n]+') | ForEach-Object Value)
    if ($runtimeLog -notmatch 'demo soft shadows: [1-9]\d* prims' -or
        $runtimeLog -notmatch '(?s)interface: UI rendering feature off\b.*?frame 650 captured to ') {
        throw 'Viewer did not record the requested fixture and world-only frame'
    }
    # Start-Process holds its stderr/stdout writer until this PowerShell step exits.
    # The next step independently checks those closed files.
    if ($runtimeLog -match 'wgpu error:|validation error|GPU device lost|panicked at') { throw "GPU or viewer error: $($Matches[0])" }
    $fullPath = Join-Path $output 'world-650.png'
    $result.pixels = Measure-Frame $fullPath
    if (!$result.pixels.visual_nonuniform) { throw 'The interior of frame 650 is undersized, blank or nearly uniform' }
    $actual = Get-Content $settingsPath -Raw | ConvertFrom-Json -AsHashtable
    if ($actual.graded_sun_shadows -ne $false -or $actual.shadow_map_resolution -ne 2048 -or
        $actual.three_level_shadow_distance -ne $reach -or $actual.shadow_quality -ne 3 -or
        !$actual.sun_shadow_contact_hardening -or $result.exe_sha256 -cne '164ee9d04fe74f392f64fe80b757fc778372f0f6036ee7f97bc9dd85ceab5ead' -or
        $runtimeLog.Contains('SwiftShader uses the three-cascade') -or $runtimeLog.Contains('new profile: graphics level')) { throw 'Unexpected settled layout, observer, or profile' }
    Record-Settings $actual (Join-Path $output 'matched-final-shadow-settings.json')
    $result.layout = @{ graded = $false; map = 2048; distance = $reach; quality = 3; contact_hardening = $true }
    $result.status = 'passed'
} catch {
    $result.detail = $_.Exception.Message
} finally {
    if ($null -ne $proc -and !$proc.HasExited) { $proc.Kill($true); [void]$proc.WaitForExit(10000) }
    if ($registered) { Remove-ItemProperty -LiteralPath $reg -Name $manifestPath }
    if ($started -and $result.status -ne 'passed') {
        Start-Sleep -Seconds 6
        $ev = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000,1001; StartTime = $started } -ErrorAction SilentlyContinue |
            Where-Object { $_.Message -match 'aurora-viewer\.exe' } |
            Select-Object TimeCreated,Id,ProviderName,Message
        $ev | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $output 'windows-application-faults.json') -Encoding utf8NoBOM
    }
    if (Test-Path -LiteralPath $log) {
        $savedLog = Join-Path $output 'viewer.log'
        Copy-Item -LiteralPath $log -Destination $savedLog
        $runtimeLog = [IO.File]::ReadAllText($savedLog)
        if ($runtimeLog -match 'GPU: ([^\r\n]+\(Vulkan, driver [^\r\n]+)') { $result.adapter = $Matches[1].Trim() }
    }
    # Every original PNG survives nonzero exit or timeout. The first request
    # is an early diagnostic; frames 605 and 650 follow the demo's UI-off action.
    $result.captures = foreach ($frameNumber in 60,605,650) {
        $filename = "world-$frameNumber.png"
        $rawPath = Join-Path $output $filename
        $entry = [ordered]@{ request_frame = $frameNumber; file = $filename; present = (Test-Path -LiteralPath $rawPath -PathType Leaf) }
        if ($entry.present) {
            try {
                $entry.sha256 = (Get-FileHash -LiteralPath $rawPath -Algorithm SHA256).Hash.ToLowerInvariant()
                $entry.pixels = Measure-Frame $rawPath
                $captureLine = [regex]::Match($runtimeLog, "\bframe $frameNumber captured to ")
                $entry.logged = $captureLine.Success
                $before = if ($captureLine.Success) { $runtimeLog.Substring(0,$captureLine.Index) } else { '' }
                $uiStates = [regex]::Matches($before,'interface: UI rendering feature (off|on)\b')
                $entry.ui_off_logged = $uiStates.Count -gt 0 -and $uiStates[$uiStates.Count-1].Groups[1].Value -eq 'off'
                $entry.world_only = $frameNumber -gt 600 -and $entry.logged -and $entry.ui_off_logged -and
                    ($before -match 'demo soft shadows: [1-9]\d* prims') -and $entry.pixels.visual_nonuniform
            } catch { $entry.measurement_error = $_.Exception.Message }
        }
        $entry
    }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'result.json') -Encoding utf8NoBOM
    Get-Content -LiteralPath (Join-Path $output 'result.json')
    "Windows portable viewer, Mesa lavapipe software Vulkan: **$($result.status)**" >> $env:GITHUB_STEP_SUMMARY
}
if ($result.status -ne 'passed') { exit 1 }
