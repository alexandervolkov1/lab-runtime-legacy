# Actual Windows GUI child, launched only by the existing Job supervisor in the
# Workbench test harness. This is qualification, never product activation.
param([ValidateSet('dev','release')][string]$BuildProfile='dev', [string]$TestFilter='parent::tests::renderer')
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    if ((& rustc -vV) -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Windows/MSVC required' }
    $directory=Join-Path $repo "target/b3b4b-qualification/native-$BuildProfile"
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $sourceHead=(& git rev-parse HEAD).Trim()
    $records=@(& cargo build -p lab-native-child --locked --profile $BuildProfile --message-format=json | ForEach-Object { $_ | ConvertFrom-Json })
    if ($LASTEXITCODE -ne 0) { $records | Where-Object reason -eq 'compiler-message' | ForEach-Object { Write-Output $_.message.rendered }; throw 'Native child build failed' }
    $records | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $directory 'cargo-artifacts.json')
    $binary=@($records | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'lab-native-child' } | ForEach-Object executable | Where-Object { $_ } | Select-Object -Unique)
    if ($binary.Count -ne 1) { throw 'Ambiguous child Cargo binary' }
    @{ source_head=$sourceHead; binary=$binary[0]; sha256=(Get-FileHash -LiteralPath $binary[0] -Algorithm SHA256).Hash; activation_qualified=$false; deployment_C=$null; test_only_job_commit_cap=1073741824 } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $directory 'identity.json')
    $receipt=Join-Path $directory ("identity-"+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffff')+'.json')
    Copy-Item -LiteralPath (Join-Path $directory 'identity.json') -Destination $receipt
    Write-Output "CHILD_IDENTITY=$receipt"
    $previous=$env:LAB_ISOLATED_RENDERER_CHILD
    try {
        $env:LAB_ISOLATED_RENDERER_CHILD=$binary[0]
        & cargo test -p lab-workbench --locked --profile $BuildProfile $TestFilter -- --ignored --test-threads=1 --nocapture
        if ($LASTEXITCODE -ne 0) { throw 'Actual isolated renderer qualification failed' }
    } finally { $env:LAB_ISOLATED_RENDERER_CHILD=$previous }
    $global:LASTEXITCODE=0
} finally { Pop-Location }
