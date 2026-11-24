# Collect diagnostics after the ordinary smoke has returned. Use only the isolated viewer's own session.
param(
    [Parameter(Mandatory)][string]$SmokeOutput,
    [Parameter(Mandatory)][string]$ViewerRoot
)
$ErrorActionPreference = 'Stop'
$result = Get-Content -LiteralPath (Join-Path $SmokeOutput 'result.json') -Raw | ConvertFrom-Json
if (!$result.process_started) { return }
$start = $null
$sessions = Join-Path $ViewerRoot 'logs/crashes/demo'
if (Test-Path -LiteralPath $sessions) {
    foreach ($session in (Get-ChildItem -LiteralPath $sessions -Directory)) {
        $metadata = Join-Path $session.FullName 'session.json'
        if (!(Test-Path -LiteralPath $metadata)) { continue }
        $record = Get-Content -LiteralPath $metadata -Raw | ConvertFrom-Json
        if ($record.pid -ne $result.pid) { continue }
        $start = [DateTimeOffset]::FromUnixTimeSeconds([int64]$record.started_unix).LocalDateTime
        $dir = Join-Path $SmokeOutput 'viewer-crash'
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        foreach ($file in @('public-report.md','native.txt','failure.txt','gpu-stall.txt','freeze.txt','panic.txt','os-events.txt')) {
            $path = Join-Path $session.FullName $file
            if ((Test-Path -LiteralPath $path -PathType Leaf) -and ((Get-Item -LiteralPath $path).Length -gt 0)) {
                Copy-Item -LiteralPath $path -Destination $dir
            }
        }
    }
}
if (!$start) {
    # Earliest process-owned stdout file is created by Start-Process before the launch.
    $start = (Get-Item -LiteralPath (Join-Path $SmokeOutput 'stdout.txt')).CreationTime
}
if ($null -ne $result.exit_code -and $result.exit_code -ne 0) { Start-Sleep -Seconds 8 }
# Wait for inherited Windows redirect handles to close before reading.
foreach ($name in @('stdout.txt','stderr.txt')) {
    $p = Join-Path $SmokeOutput $name
    $closed = $false
    for ($i = 0; $i -lt 20; $i++) {
        try {
            $stream = [IO.File]::Open($p, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
            try { $reader = [IO.StreamReader]::new($stream); $text = $reader.ReadToEnd() } finally { $stream.Dispose() }
            $closed = $true
            break
        } catch [IO.IOException] { Start-Sleep -Seconds 1 }
    }
    if (!$closed) { throw "The viewer redirect is still locked: $name" }
    if ($result.exit_code -eq 0 -and $text -match 'wgpu error:|validation error|GPU device lost|panicked at') {
        throw "Renderer error in $name, see captured evidence"
    }
}
$events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000,1001; StartTime = $start } -ErrorAction SilentlyContinue |
    Where-Object { $_.Message -match '(?i)aurora-viewer\.exe' })
$events | Format-List TimeCreated, Id, ProviderName, Message | Out-File -LiteralPath (Join-Path $SmokeOutput 'windows-errors.txt') -Encoding utf8
$details = foreach ($event in $events) {
    [xml]$xml = $event.ToXml()
    $data = [ordered]@{}
    foreach ($field in $xml.Event.EventData.Data) {
        if ($field.Name) { $data[[string]$field.Name] = [string]$field.'#text' }
    }
    [ordered]@{ event_id = $event.Id; time = $event.TimeCreated.ToUniversalTime().ToString('o'); fields = $data }
}
[ordered]@{ pid = $result.pid; start = $start.ToUniversalTime().ToString('o'); events = @($details) } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $SmokeOutput 'windows-errors.json') -Encoding utf8NoBOM
