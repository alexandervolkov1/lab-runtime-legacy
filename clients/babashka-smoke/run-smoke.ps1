[CmdletBinding()]
param(
    [ValidateRange(1, 10)][int]$Repeat = 3,
    [switch]$Presentation,
    [switch]$TwoCurveHover,
    [ValidateRange(0, 120)][int]$GuiHoldSeconds = 0,
    [string]$RuntimeExe,
    [string]$WorkbenchExe
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$bb = (Get-Command bb -ErrorAction Stop).Source
$prefix = if ($Presentation) { 'lab-dgui-3-' } else { 'lab-m17-5-' }
$temporary = Join-Path ([IO.Path]::GetTempPath()) ($prefix + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
$runtime = $null
$workbench = $null
$connectionSamples = 0
$maximumRuntimeConnections = 0
$runtimeConnectionObserved = $false

function Resolve-SmokeExecutable(
    [string]$Explicit,
    [string]$PackageName,
    [string]$DevelopmentPath
) {
    $candidate = if (-not [string]::IsNullOrWhiteSpace($Explicit)) {
        [IO.Path]::GetFullPath($Explicit)
    } elseif (Test-Path -LiteralPath (Join-Path $repo $PackageName) -PathType Leaf) {
        Join-Path $repo $PackageName
    } else {
        Join-Path $repo $DevelopmentPath
    }
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        throw "Required executable is missing: $candidate"
    }
    return [IO.Path]::GetFullPath($candidate)
}

function Start-Fixture([string]$Executable, [string[]]$Arguments, [string]$Name) {
    $info = [Diagnostics.ProcessStartInfo]::new($Executable)
    $info.WorkingDirectory = $temporary
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    # Drain stderr asynchronously so diagnostics cannot block fixture progress.
    $stderr = $process.StandardError.ReadToEndAsync()
    return @{ Process = $process; Stderr = $stderr; Name = $Name }
}

function Stop-Fixture($Fixture) {
    if ($null -eq $Fixture) { return }
    if (-not $Fixture.Process.HasExited) { $Fixture.Process.Kill($true) }
    if (-not $Fixture.Process.WaitForExit(5000)) { throw "Finite cleanup failed: $($Fixture.Name)" }
    [IO.File]::WriteAllText((Join-Path $temporary "$($Fixture.Name).stderr.txt"), $Fixture.Stderr.GetAwaiter().GetResult())
    $Fixture.Process.Dispose()
}

function Read-Ready($Fixture) {
    $line = $Fixture.Process.StandardOutput.ReadLineAsync()
    if (-not $line.Wait(10000)) { throw "Readiness deadline: $($Fixture.Name)" }
    if ([string]::IsNullOrEmpty($line.Result)) { throw "No readiness: $($Fixture.Name)" }
    return $line.Result | ConvertFrom-Json
}

function Assert-OneRuntimeConnection([int]$Port, [switch]$AllowInitialConnect) {
    # Inspect the OS TCP table, without opening a probe Runtime session.
    $connections = @([Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpConnections() |
        Where-Object { $_.LocalEndPoint.Port -eq $Port -and $_.State -eq 'Established' })
    $script:connectionSamples++
    $script:maximumRuntimeConnections = [Math]::Max($script:maximumRuntimeConnections, $connections.Count)
    # Endpoint readiness precedes worker attachment. Permit zero only during the
    # initial bounded readiness wait, before an established connection is observed.
    if ($connections.Count -eq 1) { $script:runtimeConnectionObserved = $true }
    elseif (-not $AllowInitialConnect -or $script:runtimeConnectionObserved -or $connections.Count -gt 1) {
        throw "Expected one Workbench Runtime connection, observed $($connections.Count)"
    }
}

function Get-JournalFingerprint {
    $path = Join-Path $temporary 'workspace/recovery-v1.json'
    if (Test-Path -LiteralPath $path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
    return 'absent'
}

function Invoke-Smoke([string]$Script, [int]$Port, [int]$Run, [string[]]$ModeArguments = @()) {
    $arguments = @((Join-Path $PSScriptRoot $Script), "$Port")
    if ($ModeArguments.Count -gt 0) { $arguments += $ModeArguments }
    elseif ($Presentation) { $arguments += '--presentation' }
    $fixture = Start-Fixture $bb $arguments "$Script-$Run"
    try {
        $stdout = $fixture.Process.StandardOutput.ReadToEndAsync()
        $deadline = [Diagnostics.Stopwatch]::StartNew()
        while (-not $fixture.Process.WaitForExit(50)) {
            if ($deadline.Elapsed.TotalSeconds -ge 15) { throw "Babashka deadline: $Script" }
            if ($Presentation) { Assert-OneRuntimeConnection $ready.port -AllowInitialConnect }
        }
        if ($fixture.Process.ExitCode -ne 0) { throw "Babashka failed: $($fixture.Stderr.GetAwaiter().GetResult())" }
        $output = $stdout.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $temporary "$Script-$Run.json"), $output)
        $summary = $output | ConvertFrom-Json
        if ($summary.status -ne 'PASS') { throw "Babashka omitted PASS: $Script" }
        if ($TwoCurveHover -and $ModeArguments.Count -eq 0 -and @($summary.signals).Count -ne 2) {
            throw 'Two-curve hover fixture did not expose two measurement sources'
        }
        $summary | ConvertTo-Json -Compress -Depth 20
    }
    finally { Stop-Fixture $fixture }
}

try {
    if ($GuiHoldSeconds -gt 0 -and -not $Presentation) { throw 'GUI inspection hold requires -Presentation' }
    if ($TwoCurveHover -and -not $Presentation) { throw 'Two-curve hover fixture requires -Presentation' }
    $runtimePath = Resolve-SmokeExecutable $RuntimeExe 'lab-runtime.exe' 'target/debug/lab-runtime.exe'
    $workbenchPath = Resolve-SmokeExecutable $WorkbenchExe 'lab-workbench.exe' 'target/debug/lab-workbench.exe'
    $runtimeArguments = @('--serve', '--profile', 'virtual-demo', '--port', '0')
    if ($TwoCurveHover) {
        # Two independent virtual plants with separated steady observations.
        # No controller, output command, physical device or Recorder is involved.
        $configuration = @'
schema_version = 1
[runtime]
key = "plot-hover-smoke"
display_name = "Virtual plot hover fixture"
[server]
host = "127.0.0.1"
port = 0
[recording]
enabled = false
policy = "best_effort"
'@
        foreach ($instrument in 1..2) {
            $temperature = 20 * $instrument
            $configuration += @"

[[instruments]]
id = $instrument
key = "plant-$instrument"
kind = "thermal_plant"
display_name = "Virtual plant $instrument"
history_capacity = 64
ambient_temperature = $temperature.0
initial_temperature = $temperature.0
gain_per_percent = 0.8
time_constant_ms = 8000
poll_period_ms = 100
"@
        }
        $configurationPath = Join-Path $temporary 'two-curve-hover.toml'
        [IO.File]::WriteAllText($configurationPath, $configuration)
        $runtimeArguments = @('--serve', '--config', $configurationPath)
    }
    $runtime = Start-Fixture $runtimePath $runtimeArguments 'runtime'
    $ready = Read-Ready $runtime
    if ($ready.port -lt 1) { throw 'Runtime omitted bound port' }
    $workbench = Start-Fixture $workbenchPath @(
        '--connect', "127.0.0.1:$($ready.port)", '--workspace', (Join-Path $temporary 'workspace'),
        '--workbench-listen', '127.0.0.1:0'
    ) 'workbench'
    $endpoint = [Net.IPEndPoint]::Parse((Read-Ready $workbench).workbench_endpoint)
    if ($endpoint.Address.ToString() -ne '127.0.0.1' -or $endpoint.Port -lt 1) { throw 'Unexpected Workbench address' }
    foreach ($run in 1..$Repeat) {
        # The declarative scenario has exactly one Runtime client: Workbench.
        if (-not $Presentation) { Invoke-Smoke 'runtime.clj' $ready.port $run }
        $journalBefore = Get-JournalFingerprint
        Invoke-Smoke 'workbench.clj' $endpoint.Port $run
        if ($Presentation) {
            Assert-OneRuntimeConnection $ready.port
            if ((Get-JournalFingerprint) -ne $journalBefore) { throw 'Presentation changed the recovery journal' }
        }
        if ($runtime.Process.HasExited -or $workbench.Process.HasExited) { throw 'Caller exit stopped a fixture' }
    }
    if ($Presentation) {
        $workbench.Process.Refresh()
        if ($workbench.Process.MainWindowHandle -eq 0) { throw 'No native Workbench window observed' }
        Write-Output "Native window observed (not a visual assertion); Workbench PID $($workbench.Process.Id); Runtime PID $($runtime.Process.Id); artifacts: $temporary"
        # Optional finite inspection interval. It does not claim that a human or
        # screenshot has verified tab switching, values, or plotted pixels.
        $hold = [Diagnostics.Stopwatch]::StartNew()
        while ($hold.Elapsed.TotalSeconds -lt $GuiHoldSeconds) {
            Assert-OneRuntimeConnection $ready.port
            if ($runtime.Process.HasExited -or $workbench.Process.HasExited) { throw 'Fixture exited during inspection' }
            Start-Sleep -Milliseconds 100
        }
        Invoke-Smoke 'workbench.clj' $endpoint.Port ($Repeat + 1) @(
            '--verify-presentation', (Join-Path $temporary "workbench.clj-$Repeat.json")
        )
        Assert-OneRuntimeConnection $ready.port
        if ((Get-JournalFingerprint) -ne $journalBefore) { throw 'GUI inspection changed the recovery journal' }
        if (Test-Path -LiteralPath (Join-Path $temporary 'workspace/presentation-v1.json')) {
            throw 'In-memory replacement unexpectedly persisted a presentation file'
        }
        Write-Output "Runtime TCP samples: $connectionSamples; maximum established connections: $maximumRuntimeConnections; recovery journal unchanged"
        [IO.File]::WriteAllText((Join-Path $temporary 'native-process-evidence.json'), (@{
            native_window_observed = $true
            visual_assertions = $false
            runtime_connection_samples = $connectionSamples
            maximum_runtime_connections = $maximumRuntimeConnections
            recovery_journal_unchanged = $true
            presentation_file_created = $false
            inspection_seconds = $GuiHoldSeconds
        } | ConvertTo-Json))
    }
    Stop-Fixture $workbench
    $workbench = $null
    if ($runtime.Process.HasExited) { throw 'Workbench termination stopped Runtime' }
    $scenario = if ($Presentation) { 'declarative Workbench' } else { 'direct Runtime + Workbench' }
    Write-Output "PASS: $Repeat $scenario Babashka runs; finite cleanup; logs: $temporary"
}
finally {
    try { Stop-Fixture $workbench }
    finally { Stop-Fixture $runtime }
}
