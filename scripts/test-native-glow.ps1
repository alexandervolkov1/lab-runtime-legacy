# Native GL smoke is deliberately separate from the allocation-meter processes.
param([ValidateSet('dev','release')][string]$BuildProfile = 'dev', [switch]$Pending, [switch]$Faults, [switch]$CompileOnly)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    if ((& rustc -vV) -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Windows/MSVC required' }
    $records = & cargo build -p lab-workbench --locked --profile $BuildProfile --message-format=json | ForEach-Object { $_ | ConvertFrom-Json }
    if ($LASTEXITCODE -ne 0) { throw 'Workbench artifact build failed' }
    $artifacts = @($records | Where-Object reason -eq 'compiler-artifact')
    $metadata = (& cargo metadata --no-deps --locked --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Metadata failed' }
    $package = $metadata.packages | Where-Object name -eq 'lab-workbench'
    $dependencies = @($package.dependencies | Where-Object { $null -eq $_.kind } | ForEach-Object { $_.name.Replace('-','_') })
    # Explicit smoke inputs, already present at these exact versions in Cargo.lock.
    # No new dependency resolution, features, workspace members or profile changes.
    $native = @{ glutin = '0.32.3'; glutin_winit = '0.5.0'; winit = '0.30.13'; raw_window_handle = '0.6.2' }
    $externs = @()
    foreach ($name in ($dependencies + @($native.Keys))) {
        $requiredFeatures = @($package.dependencies | Where-Object { $_.name.Replace('-','_') -eq $name -and $null -eq $_.kind } | ForEach-Object features)
        $artifact = @($artifacts | Where-Object {
            $candidate = $_
            $candidate.target.name -eq $name -and @($requiredFeatures | Where-Object { $candidate.features -notcontains $_ }).Count -eq 0
        })
        $files = @($artifact | ForEach-Object filenames | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous native artifact $name" }
        if ($native.ContainsKey($name) -and $artifact[0].package_id -notlike "*@$($native[$name])") { throw "Unreviewed native version $name" }
        $externs += @('--extern', "$name=$($files[0])")
    }
    foreach ($path in @($records | Where-Object reason -eq 'build-script-executed' | ForEach-Object linked_paths | Select-Object -Unique)) { $externs += @('-L', $path) }
    $probeDir = Join-Path $repo "target/native-glow/$BuildProfile"
    New-Item -ItemType Directory -Path $probeDir -Force | Out-Null
    $sourceRoot = (Join-Path $repo 'apps/lab-workbench/src').Replace('\','/')
    $support = (Join-Path $repo 'scripts/support').Replace('\','/')
    $modules = @()
    foreach ($name in @('client','dispatcher','external','readonly_limits','readonly_values','readonly_cache','model','ownership','presentation','recovery','storage')) {
        $path = "$sourceRoot/$name.rs"
        if (-not (Test-Path -LiteralPath $path)) { $path = "$sourceRoot/$name/mod.rs" }
        $modules += "#[path = `"$path`"] mod $name;"
    }
    $source = Join-Path $probeDir 'main.rs'
    $wrapper = @"
//! Standalone native rendering qualification; no Runtime connection.
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
$($modules -join "`n")
#[path = "$sourceRoot/gui/rebuild.rs"] mod rebuild;
mod gui { #[path = "$sourceRoot/gui/readonly_v2/mod.rs"] pub(crate) mod readonly_v2; }
use gui::readonly_v2;
#[path = "$sourceRoot/gui/readonly_v2/plot_fixtures.rs"] mod plot_fixtures;
#[path = "$support/native-glow-smoke.rs"] mod native_glow;
fn main() { native_glow::run(); }
"@
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $directory = if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
    $flags = @('-C','debuginfo=2') + $(if ($BuildProfile -eq 'dev') { @('-C','debug-assertions=yes','-C','overflow-checks=yes') } else { @('-C','opt-level=3','-C','debug-assertions=no','-C','overflow-checks=no') })
    $binary = Join-Path $probeDir 'native-glow.exe'
    $map = Join-Path $probeDir 'native-glow.map'
    $flags += @('-C', ('link-arg=/MAP:' + $map))
    & rustc --edition=2024 -D warnings $source @externs @flags -L "dependency=$(Join-Path $repo "target/$directory/deps")" -o $binary
    if ($LASTEXITCODE -ne 0) { throw 'Native GL launcher compilation failed' }
    & clippy-driver --edition=2024 -D warnings -W clippy::all -W missing-docs --emit=metadata $source @externs @flags -L "dependency=$(Join-Path $repo "target/$directory/deps")" -o (Join-Path $probeDir 'clippy.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Native GL launcher Clippy failed' }
    $artifacts | Where-Object { $_.target.name -in @('eframe','egui','glow','glutin','glutin_winit','winit','raw_window_handle') } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $probeDir 'artifacts.json')
    if ($CompileOnly) {
        if ($Pending -or $Faults) { throw 'CompileOnly cannot qualify a native execution scenario' }
        Write-Output 'B2B2 standalone compile and Clippy PASS; no GPU execution or child integration claim'
        return
    }
    $identity=@{ source_head=(& git rev-parse HEAD).Trim(); binary=$binary; sha256=(Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash; pending=[bool]$Pending; faults=[bool]$Faults; integration=$false }
    $receipt=Join-Path $probeDir ("identity-"+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffff')+'.json')
    $identity | ConvertTo-Json | Set-Content -LiteralPath $receipt
    Write-Output "STANDALONE_IDENTITY=$receipt"
    # A retained unresolved root must NOT unwind normally. A separate watchdog
    # observes it and terminates only this child; that is BLOCKED, never cleanup.
    $scenarios = if ($Faults) { @('timeout-proof','wait-failed','missing-fence','invalid-fence','device-loss','partial-texture','original-error','native-teardown') + @('texture','buffer','shader','program','vao','fence' | ForEach-Object { "$($_)-before"; "$($_)-after" }) } elseif ($Pending) { @('pending') } else { @('smoke') }
    foreach ($scenario in $scenarios) {
        $out = Join-Path $probeDir "$scenario.txt"
        $err = Join-Path $probeDir "$scenario-stderr.txt"
        $launch = @{ FilePath=$binary; PassThru=$true; WindowStyle='Hidden'; RedirectStandardOutput=$out; RedirectStandardError=$err }
        if ($scenario -eq 'pending') { $launch.ArgumentList = '--pending' }
        elseif ($scenario -ne 'smoke') { $launch.ArgumentList = "--fault=$scenario" }
        $process = Start-Process @launch
        $deadline = [Diagnostics.Stopwatch]::StartNew()
        try {
            do {
                Start-Sleep -Milliseconds 100
                [string]$runText = Get-Content -LiteralPath $out -Raw
                $process.Refresh()
            } while (-not $process.HasExited -and $runText -notmatch 'PENDING_READY|TERMINAL GPU LIFECYCLE BLOCKED:.*native root retained' -and $deadline.Elapsed.TotalSeconds -lt 30)
            if ($process.HasExited) { $process.WaitForExit() }
            [string]$runText = Get-Content -LiteralPath $out -Raw
            $runText
            if ($runText -match 'RUST_RESIDUAL_ATTRIBUTION') {
                $mapped = Join-Path $probeDir "$scenario-attribution.txt"
                & (Join-Path $PSScriptRoot 'resolve-native-allocations.ps1') -Map $map -Transcript $out | Set-Content -LiteralPath $mapped
                Get-Content -LiteralPath $mapped
            }
            Get-Content -LiteralPath $err
            if ($Faults -and $scenario -ne 'timeout-proof') {
                if ($process.HasExited -or $runText -notmatch 'PENDING_READY current=true' -or $runText -notmatch "FAULT $scenario" -or $runText -notmatch 'gl_errors: 0') { throw "Fault ownership witness failed: $scenario" }
                if ($scenario -eq 'native-teardown' -and $runText -notmatch 'objects: 0,') { throw 'Native teardown injection retained GPU objects' }
                Write-Output "FAULT $scenario containment witnessed; TERMINAL GPU LIFECYCLE BLOCKED; external termination is not cleanup"
            } elseif ($scenario -eq 'timeout-proof') {
                if (-not $process.HasExited -or $process.ExitCode -ne 0 -or $runText -notmatch 'FAULT timeout-proof PASS') { throw 'Real later completion proof failed' }
            } elseif ($Pending) {
                if ($process.HasExited -or $runText -notmatch 'PENDING_READY' -or $runText -notmatch 'keys=1, context=true, frame=true') { throw 'Pending ownership witness failed' }
                Write-Output 'External process termination follows: TERMINAL GPU LIFECYCLE BLOCKED (no teardown receipt)'
            } elseif (-not $process.HasExited -or $process.ExitCode -ne 0 -or $runText -notmatch 'GPU RENDER SMOKE PASS; HEALTHY GPU LIFECYCLE PASS') {
                throw 'Native GPU smoke failed or remained unresolved; external termination is not cleanup'
            }
        } finally {
            $process.Refresh()
            if (-not $process.HasExited) { Stop-Process -Id $process.Id }
        }
    }
} finally { Pop-Location }
