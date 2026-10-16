# Finite CPU-only MP-1 experiments. Every invocation requires a new output directory.
param(
    [ValidateSet('dev','release')][string]$BuildProfile = 'dev',
    [Parameter(Mandatory)][string]$Directory,
    [string]$ProofExecutable
)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathFullyQualified($Directory) -or (Test-Path -LiteralPath $Directory)) {
    throw 'A new absolute evidence directory is required'
}
[void][IO.Directory]::CreateDirectory($Directory)
$repo = (Resolve-Path "$PSScriptRoot/../../..").Path
$manifest = "$PSScriptRoot/Cargo.toml"
$profileDirectory = if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
$binaries = @()
foreach ($subsystem in @('console','windowed')) {
    $build = "$Directory/build-$subsystem"
    $cargoArguments = @('build','--locked','--manifest-path',$manifest,'--profile',$BuildProfile,'--target-dir',$build)
    if ($subsystem -eq 'windowed') { $cargoArguments += @('--features','windowed') }
    & cargo @cargoArguments 2>&1 | Tee-Object -FilePath "$Directory/build-$subsystem.log"
    if ($LASTEXITCODE -ne 0) { throw 'Probe build failed' }
    $binary = "$build/$profileDirectory/lab-job-accounting.exe"
    $binaries += [ordered]@{ subsystem=$subsystem; path=$binary; bytes=(Get-Item -LiteralPath $binary).Length; sha256=(Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$controller = $binaries[0].path
# Persist identities before any process can fail; retain failures as evidence.
$binaries | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$Directory/binary-identities.json" -Encoding utf8
foreach ($binary in $binaries) {
    & $controller --controller "$Directory/cpu-$($binary.subsystem)" $binary.path 2>&1 | Tee-Object -FilePath "$Directory/run-$($binary.subsystem).log"
    if ($LASTEXITCODE -ne 0) { throw 'CPU experiment failed; evidence retained' }
}
$verification = @("$PSScriptRoot/verify.py", "$Directory/cpu-console", "$Directory/cpu-windowed")
if ($ProofExecutable) {
    $proof = (Resolve-Path -LiteralPath $ProofExecutable).Path
    $binaries += [ordered]@{ subsystem='historical-render-2a'; path=$proof; bytes=(Get-Item -LiteralPath $proof).Length; sha256=(Get-FileHash -LiteralPath $proof -Algorithm SHA256).Hash.ToLowerInvariant() }
    $binaries | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$Directory/binary-identities.json" -Encoding utf8
    & $controller --graphical $proof "$Directory/graphical" 2>&1 | Tee-Object -FilePath "$Directory/run-graphical.log"
    if ($LASTEXITCODE -ne 0) { throw 'Graphical experiment failed; evidence retained' }
    $verification += "$Directory/graphical"
}
$windowsVersion = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$environment = [ordered]@{
    source = (& git -C $repo rev-parse HEAD)
    source_status = @(& git -C $repo status --porcelain)
    profile = $BuildProfile
    utc = [DateTime]::UtcNow.ToString('o')
    windows = [ordered]@{ product=$windowsVersion.ProductName; build=$windowsVersion.CurrentBuild; ubr=$windowsVersion.UBR; version=$windowsVersion.DisplayVersion }
    os_architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    process_architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    processor = @(Get-CimInstance Win32_Processor | Select-Object Name,Architecture)
    graphics = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,PNPDeviceID)
    rustc = @(& rustc -vV)
    cargo = (& cargo --version)
    binaries = $binaries
}
$environment | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$Directory/environment.json" -Encoding utf8
& python @verification | Tee-Object -FilePath "$Directory/verification.json"
if ($LASTEXITCODE -ne 0) { throw 'Observation verification failed; evidence retained' }
