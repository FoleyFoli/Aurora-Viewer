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
    requested_driver = 'Chrome SwiftShader (software Vulkan)'
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
$proc = $null
try {
    $zip = Join-Path $env:GITHUB_WORKSPACE "test-client/$name.zip"
    $sum = (Get-Content -LiteralPath "$zip.sha256" -Raw).Trim()
    $result.zip_sha256 = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($result.zip_sha256 -cne $env:EXPECTED_ZIP_SHA256) { throw 'Portable ZIP differs from the digest exported by the current package job' }
    if ($sum -cne "$($result.zip_sha256)  $name.zip") { throw 'Portable ZIP checksum mismatch' }
    New-Item -ItemType Directory -Path $root | Out-Null
    Expand-Archive -LiteralPath $zip -DestinationPath $root
    Rename-Item -LiteralPath (Join-Path $root $name) -NewName 'install'
    $manifest = Get-Content -LiteralPath (Join-Path $install 'build-manifest.json') -Raw | ConvertFrom-Json
    $exe = Join-Path $install 'aurora-viewer.exe'
    $result.exe_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($result.exe_sha256 -cne $env:EXPECTED_EXE_SHA256) { throw 'Extracted EXE differs from the digest exported by the current package job' }
    if ($manifest.source_commit -cne $env:SOURCE_COMMIT -or $manifest.source_tree -cne $env:SOURCE_TREE -or
        $manifest.workflow_commit -cne $buildWorkflowCommit -or
        $manifest.workflow_run -cne "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$buildRunId" -or
        $manifest.executable.sha256 -cne $result.exe_sha256) {
        throw 'Packaged executable or source identity differs from the build manifest'
    }
    if ($env:WINDOWS_SMOKE_DRIVER -ne 'chrome-swiftshader') { throw 'Explicit software-driver opt-in is required' }
    # SwiftShader documents app-local vulkan-1.dll for direct driver loading.
    # This affects only the extracted test copy and requires no driver installation.
    $roots = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } | Select-Object -Unique
    $drivers = foreach ($base in $roots) {
        $app = Join-Path $base 'Google/Chrome/Application'
        if (Test-Path -LiteralPath $app) {
            Get-ChildItem -LiteralPath $app -Directory |
                Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
                ForEach-Object {
                    $dll = Join-Path $_.FullName 'vk_swiftshader.dll'
                    if (Test-Path -LiteralPath $dll -PathType Leaf) {
                        [PSCustomObject]@{ Version = [version]$_.Name; File = $dll }
                    }
                }
        }
    }
    $driver = $drivers | Sort-Object Version -Descending | Select-Object -First 1
    if (!$driver) { throw 'No preinstalled Chrome SwiftShader runtime on the runner' }
    $target = Join-Path $install 'vulkan-1.dll'
    if (Test-Path -LiteralPath $target) { throw 'Packaged archive already includes a Vulkan loader' }
    Copy-Item -LiteralPath $driver.File -Destination $target
    $result.software_driver = @{
        browser_version = $driver.Version.ToString()
        source = $driver.File
        sha256 = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
        runtime_name = 'vulkan-1.dll'
    }
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
    $env:AURORA_CAPTURE_FRAMES = '650'
    $env:AURORA_CAPTURE_EXIT = '1'
    $env:RUST_LOG = 'info'
    $proc = Start-Process -FilePath $exe -ArgumentList '--title','Test-Windows-Shading','--language','en' -WorkingDirectory $install -PassThru -RedirectStandardOutput (Join-Path $output 'stdout.txt') -RedirectStandardError (Join-Path $output 'stderr.txt')
    $result.process_started = $true
    $result.pid = $proc.Id
    $noAdapter = 'Vulkan lists no graphics card after all retries|no native GPU adapter can present to the window'
    $limit = [DateTime]::UtcNow.AddMinutes(45)
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
    if ($result.adapter -notmatch 'SwiftShader') { throw 'Viewer did not confirm the requested software Vulkan adapter' }
    $result.adapter_candidates = @([regex]::Matches($runtimeLog, 'GPU adapter \d+: [^\r\n]+') | ForEach-Object Value)
    if ($runtimeLog -notmatch 'demo soft shadows: [1-9]\d* prims' -or
        $runtimeLog -notmatch '(?s)interface: UI rendering feature off\b.*?frame 650 captured to ') {
        throw 'Viewer did not record the requested fixture and world-only frame'
    }
    $messages = $runtimeLog + [IO.File]::ReadAllText((Join-Path $output 'stderr.txt')) + [IO.File]::ReadAllText((Join-Path $output 'stdout.txt'))
    if ($messages -match 'wgpu error:|validation error|GPU device lost|panicked at') { throw "GPU or viewer error: $($Matches[0])" }
    Add-Type -AssemblyName System.Drawing
    $image = [Drawing.Bitmap]::new($frame)
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
        $result.pixels = @{ width = $image.Width; height = $image.Height; samples = $count; nonblack = $light; rgb_444_bins = $colors.Count }
        if ($image.Width -lt 400 -or $image.Height -lt 280 -or $light -lt $count * 0.5 -or $colors.Count -lt 16) {
            throw 'The interior of the world capture is undersized, blank or nearly uniform'
        }
    } finally { $image.Dispose() }
    $result.status = 'passed'
} catch {
    $result.detail = $_.Exception.Message
} finally {
    if ($null -ne $proc -and !$proc.HasExited) { $proc.Kill($true); [void]$proc.WaitForExit(10000) }
    if (Test-Path -LiteralPath $log) { Copy-Item -LiteralPath $log -Destination (Join-Path $output 'viewer.log') }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $output 'result.json') -Encoding utf8NoBOM
    Get-Content -LiteralPath (Join-Path $output 'result.json')
    "Windows portable viewer, Chrome SwiftShader software Vulkan: **$($result.status)**" >> $env:GITHUB_STEP_SUMMARY
}
if ($result.status -ne 'passed') { exit 1 }
