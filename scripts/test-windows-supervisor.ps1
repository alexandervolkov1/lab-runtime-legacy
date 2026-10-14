# Windows/MSVC no-GPU acceptance through the production supervisor seam.
param([ValidateSet('dev','release')][string]$BuildProfile = 'dev', [switch]$FixtureOnly)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$runDirectory = Join-Path $repo ('target/windows-supervisor/' + $BuildProfile + '/' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($runDirectory)
Push-Location $repo
try {
    $version = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $version -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Windows x64 MSVC required' }
    $records = & cargo build -p lab-windows-job --features fixture-probes --locked --profile $BuildProfile --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Capsule build failed' }
    $artifacts = @($records | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'lab_windows_job' })
    $rlib = @($artifacts | ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' })
    if ($rlib.Count -ne 1) { throw 'Ambiguous capsule artifact' }
    $profileDirectory = if ($BuildProfile -eq 'release') { 'release' } else { 'debug' }
    $flags = if ($BuildProfile -eq 'release') { @('-C','opt-level=3') } else { @('-C','debug-assertions=yes','-C','overflow-checks=yes') }
    $fixture = Join-Path $runDirectory 'supervisor-fixture.exe'
    $source = Join-Path $PSScriptRoot 'support/windows-supervisor-fixture.rs'
    & rustc --edition=2024 -D warnings -W missing-docs $source @flags --extern "lab_windows_job=$($rlib[0])" -L "dependency=$(Join-Path $repo "target/$profileDirectory/deps")" -o $fixture
    if ($LASTEXITCODE -ne 0) { throw 'Fixture build failed' }
    & clippy-driver --edition=2024 -D warnings -W clippy::all -W missing-docs --emit=metadata $source @flags --extern "lab_windows_job=$($rlib[0])" -L "dependency=$(Join-Path $repo "target/$profileDirectory/deps")" -o (Join-Path $runDirectory 'fixture-clippy.rmeta')
    if ($LASTEXITCODE -ne 0) { throw 'Fixture Clippy failed' }
    Write-Output "FIXTURE=$fixture"
    if (-not $FixtureOnly) {
        $oldFixture = $env:LAB_WINDOWS_SUPERVISOR_FIXTURE
        try {
            $env:LAB_WINDOWS_SUPERVISOR_FIXTURE = $fixture
            $args = @('test','-p','lab-workbench','--locked','--profile',$BuildProfile,'--','supervisor::native_tests','--ignored','--test-threads=1','--nocapture')
            & cargo @args
            if ($LASTEXITCODE -ne 0) { throw 'Native supervisor acceptance failed' }
        } finally { $env:LAB_WINDOWS_SUPERVISOR_FIXTURE = $oldFixture }
    }
} finally { Pop-Location }
