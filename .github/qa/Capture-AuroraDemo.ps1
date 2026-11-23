#requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Viewer,
    [Parameter(Mandatory)][ValidateSet('ShadowSlope','Focus','Stochastic','AlphaPlanes','MixedMaterials','Plaza')][string]$Scene,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9][A-Za-z0-9_-]*$')][string]$Name,
    [ValidateRange(-1,7)][int]$AA = 6,
    [ValidateRange(0,4)][int]$FsrQuality = 0,
    [string]$OutputDirectory = (Join-Path (Get-Location) '.scratch/pr-captures'),
    [ValidatePattern('^[1-9][0-9]{2,3},[1-9][0-9]{2,3}$')][string]$WindowSize = '1280,720',
    [ValidateRange(1,7200)][int]$TimeoutSeconds = 900,
    [switch]$Interactive
)

$ErrorActionPreference = 'Stop'
$original = Get-Item -LiteralPath $Viewer
if ($original.PSIsContainer) { throw "Expected the path of a built viewer executable." }
$frames = [IO.Directory]::CreateDirectory($OutputDirectory).FullName
$image = Join-Path $frames "$Name.png"
$outputLog = Join-Path $frames "$Name.log"
if (!$Interactive -and ((Test-Path -LiteralPath $image) -or (Test-Path -LiteralPath $outputLog))) {
    throw "Capture files already exist for $Name. Choose a new name."
}

# Aurora checks the executable location, marker schema and loopback feed before
# selecting throwaway settings and disabling access to the credential store.
$root = Join-Path ([IO.Path]::GetTempPath()) ("aurora-pr-demo-" + [Guid]::NewGuid().ToString('N'))
$install = [IO.Directory]::CreateDirectory((Join-Path $root 'install')).FullName
$exe = Join-Path $install $original.Name
Copy-Item -LiteralPath $original.FullName -Destination $exe
Get-ChildItem -LiteralPath $original.DirectoryName -File -Filter '*.dll' | Copy-Item -Destination $install
$assets = Join-Path $original.DirectoryName 'assets'
if (Test-Path -LiteralPath $assets -PathType Container) { Copy-Item -LiteralPath $assets -Destination $install -Recurse }
# Capture-only Ed25519 public key. The private key was discarded. The marker
# is read only by the executable inside this temporary loopback installation.
$marker = @{
    schema = 1
    feed = 'http://127.0.0.1:1/feed.json'
    public_key = '64b9f63e39ac482952f1ec5fb675b62b2331b272951e8292f5394dd84e099b7c'
}
[IO.File]::WriteAllText((Join-Path $root 'aurora-update-test.json'), ($marker | ConvertTo-Json -Compress))

$info = [Diagnostics.ProcessStartInfo]::new()
$info.FileName = $exe
$info.WorkingDirectory = $install
$info.UseShellExecute = $false
$info.ArgumentList.Add('--title')
$info.ArgumentList.Add("Test rendering $Scene")
$info.ArgumentList.Add('--language')
$info.ArgumentList.Add('en')
foreach ($key in @($info.Environment.Keys)) {
    if ($key -like 'AURORA_*') { [void]$info.Environment.Remove($key) }
}
$settings = @{
    AURORA_DEMO='1'; AURORA_UPDATE_TEST_ROOT=$root; AURORA_WINDOW_SIZE=$WindowSize; AURORA_LOG_NAME='rendering-demo'
}
if ($AA -ge 0) { $settings.AURORA_AA="$AA" }
if ($AA -eq 7) { $settings.AURORA_FSR_QUALITY="$FsrQuality" }
switch ($Scene) {
    'ShadowSlope' { $settings.AURORA_DEMO_POS='132,126'; $settings.AURORA_DEMO_SOFT_SHADOWS='slope'; $settings.AURORA_DEMO_SUN='27,5' }
    'Focus' { $settings.AURORA_DEMO_DOF='1.4:3.3'; $settings.AURORA_DEMO_HAIR_CARDS='12' }
    'Stochastic' { $settings.AURORA_DEMO_ALPHA_PLANES='8,0.4,0.5'; $settings.AURORA_OIT='1' }
    'AlphaPlanes' { $settings.AURORA_DEMO_ALPHA_PLANES='8,0.4,0.5' }
    'MixedMaterials' { $settings.AURORA_DEMO_PBR_MIXED='1' }
}
if (!$Interactive) {
    $settings.AURORA_CAPTURE=Join-Path $root 'capture.png'
    $settings.AURORA_CAPTURE_FRAMES=if ($Scene -eq 'Focus') { '780' } else { '620' }
    $settings.AURORA_CAPTURE_EXIT='1'
}
foreach ($entry in $settings.GetEnumerator()) { $info.Environment[$entry.Key] = $entry.Value }
try {
    $run = [Diagnostics.Process]::Start($info)
    if ($Interactive) {
        $run.WaitForExit()
    } elseif (!$run.WaitForExit(1000 * $TimeoutSeconds)) {
        $run.Kill($true)
        $run.WaitForExit()
        throw "The $Scene demo exceeded its $TimeoutSeconds second capture timeout."
    }
    if ($run.ExitCode -ne 0) { throw "Viewer exited with status $($run.ExitCode)." }
    if (!$Interactive) {
        $log = Join-Path $root 'logs/rendering-demo.log'
        [IO.File]::Copy($log, $outputLog)
        if (Select-String -LiteralPath $log -SimpleMatch 'wgpu error' -Quiet) {
            throw "Renderer reported a wgpu error in $outputLog."
        }
        $bytes = [IO.File]::ReadAllBytes($settings.AURORA_CAPTURE)
        if ($bytes.Length -lt 24 -or [BitConverter]::ToString($bytes,0,8) -ne '89-50-4E-47-0D-0A-1A-0A') {
            throw "Viewer output is not a PNG image."
        }
        [IO.File]::Copy($settings.AURORA_CAPTURE, $image)
        Write-Output $image
    }
} catch {
    Write-Warning "The temporary test installation has been kept at $root."
    throw
}
Remove-Item -LiteralPath $root -Recurse -Force
