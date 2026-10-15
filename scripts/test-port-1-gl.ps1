# Explicit, bounded PORT-1 experiment. Hardware names are diagnostics only.
# This calls the original B3B4C harness; it never selects a product GUI host.
param(
    [ValidateSet('dev','release')][string]$BuildProfile='dev',
    [Parameter(Mandatory)][ValidateRange(16777216,1073741824)][long]$TestCommitCap,
    [string]$TestFilter='parent::tests::renderer'
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    if ((& rustc -vV) -notcontains 'host: x86_64-pc-windows-msvc') { throw 'PORT-1 native experiment requires Windows/MSVC' }
    $sourceChanges=@(& git status --porcelain --untracked-files=normal -- apps crates scripts Cargo.toml Cargo.lock)
    if ($LASTEXITCODE -ne 0 -or $sourceChanges.Count -ne 0) { throw 'Commit experiment source inputs before qualification; no source identity inferred from a dirty tree' }
    $started=[DateTime]::UtcNow
    $directory=Join-Path $repo ("target/port-1/$BuildProfile-"+$started.ToString('yyyyMMddTHHmmssfffffff'))
    New-Item -ItemType Directory -Path $directory | Out-Null
    $identity=[ordered]@{
        source_head=(& git rev-parse HEAD).Trim()
        toolchain=@(& rustc -vV)
        test_only_commit_cap=$TestCommitCap
        deployment_C=$null
        activation_qualified=$false
        hardware_is_admission_input=$false
        historical_224_MiB_qualification_transferred=$false
    }
    $identity | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $directory 'experiment.json')
    $fingerprint=[ordered]@{}
    foreach ($class in @('Win32_OperatingSystem','Win32_Processor','Win32_VideoController')) {
        # Diagnostic availability/fingerprint never decides graphics admission.
        try {
            $properties=switch ($class) {
                'Win32_OperatingSystem' { @('Caption','Version','OSArchitecture') }
                'Win32_Processor' { @('Name','NumberOfCores','NumberOfLogicalProcessors') }
                'Win32_VideoController' { @('Name','DriverVersion','PNPDeviceID','VideoModeDescription') }
            }
            $fingerprint[$class]=@(Get-CimInstance -ClassName $class | Select-Object -Property $properties)
        } catch { $fingerprint[$class]=@{diagnostic_error=$_.ToString()} }
    }
    $fingerprint | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $directory 'hardware-diagnostics.json')
    $failure=$null
    try {
        & (Join-Path $PSScriptRoot 'test-isolated-renderer.ps1') -BuildProfile $BuildProfile -QualificationCommitCap $TestCommitCap -TestFilter $TestFilter *>&1 |
            Tee-Object -FilePath (Join-Path $directory 'native-transcript.txt')
    } catch { $failure=$_.ToString() }
    $target=if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
    $nativeRoot=Join-Path $repo "target/$target/target/isolated-native-renderer"
    $incarnations=@()
    if (Test-Path -LiteralPath $nativeRoot) {
        $incarnations=@(Get-ChildItem -LiteralPath $nativeRoot -Directory | Where-Object CreationTimeUtc -ge $started)
    }
    $manifest=@(foreach ($incarnation in $incarnations) {
        foreach ($file in Get-ChildItem -LiteralPath $incarnation.FullName -File) {
            [ordered]@{path=$file.FullName; bytes=$file.Length; sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash}
        }
    })
    $manifest | ConvertTo-Json -Depth 8 -AsArray | Set-Content -LiteralPath (Join-Path $directory 'native-files.json')
    $originalIdentity=Join-Path $repo "target/b3b4c-qualification/native-$BuildProfile/identity.json"
    if (Test-Path -LiteralPath $originalIdentity) {
        Copy-Item -LiteralPath $originalIdentity -Destination (Join-Path $directory 'child-identity.json')
    }
    [ordered]@{source_head=$identity.source_head; test_filter=$TestFilter; test_only_commit_cap=$TestCommitCap;
        incarnation_directories=$incarnations.Count; native_files=$manifest.Count; failure=$failure;
        verdict=$(if ($failure) {'FAIL'} else {'PASS'}); deployment_C=$null; activation_qualified=$false} |
        ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $directory 'result.json')
    Write-Output "PORT1_EVIDENCE=$directory"
    if ($failure) { throw $failure }
    $global:LASTEXITCODE=0
} finally { Pop-Location }
