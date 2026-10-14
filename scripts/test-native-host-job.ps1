# Isolated Windows x64 OS preflight; no GUI, GL or Runtime process is launched.
param([ValidateSet('dev','release')][string]$BuildProfile = 'dev')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$runDirectory = Join-Path $repo ('target/native-host-job/' + $BuildProfile + '/' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($runDirectory)
$source = Join-Path $PSScriptRoot 'support/native-host-job-probe.rs'
$binary = Join-Path $runDirectory 'native-host-job.exe'
$flags = if ($BuildProfile -eq 'release') { @('-C','opt-level=3') } else { @('-C','debug-assertions=yes','-C','overflow-checks=yes') }
& rustc --edition=2024 -D warnings -W missing-docs $source @flags -o $binary
if ($LASTEXITCODE -ne 0) { throw 'Job preflight compilation failed' }
& clippy-driver --edition=2024 -D warnings -W clippy::all -W missing-docs --emit=metadata $source @flags -o (Join-Path $runDirectory 'clippy.rmeta')
if ($LASTEXITCODE -ne 0) { throw 'Job preflight Clippy failed' }
$out = Join-Path $runDirectory 'probe.txt'
$err = Join-Path $runDirectory 'stderr.txt'
$process = Start-Process -FilePath $binary -ArgumentList ('"' + $runDirectory + '"') -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError $err
try {
    if (-not $process.WaitForExit(15000)) { throw 'OS preflight external watchdog timeout; no termination/cleanup qualification' }
    Get-Content -LiteralPath $out
    Get-Content -LiteralPath $err
    if ($process.ExitCode -ne 0 -or (Get-Content -LiteralPath $out -Raw) -notmatch 'NATIVE HOST JOB PREFLIGHT PASS') { throw 'Job preflight BLOCKED' }
    Write-Output "PROFILE=$BuildProfile LOG=$out"
} finally {
    $process.Refresh()
    if (-not $process.HasExited) { Stop-Process -Id $process.Id }
}
