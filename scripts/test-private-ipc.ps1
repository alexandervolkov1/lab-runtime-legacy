param([ValidateSet('dev', 'release')][string]$BuildProfile = 'dev', [switch]$FixtureOnly)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    $toolchain = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $toolchain -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Private IPC qualification requires Windows x64/MSVC' }
    $artifacts = & cargo build -p lab-workbench --locked --profile $BuildProfile --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Workbench build failed' }
    $records = @($artifacts | ForEach-Object { $_ | ConvertFrom-Json })
    $compiled = @($records | Where-Object { $_.reason -eq 'compiler-artifact' })
    $metadata = (& cargo metadata --no-deps --locked --format-version 1 | ConvertFrom-Json)
    $package = $metadata.packages | Where-Object { $_.name -eq 'lab-workbench' }
    $externs = @()
    foreach ($dependency in $package.dependencies | Where-Object { $null -eq $_.kind }) {
        $name = $dependency.name.Replace('-', '_')
        $files = @($compiled | Where-Object {
            $artifact = $_
            $artifact.target.name -eq $name -and @($dependency.features | Where-Object { $artifact.features -notcontains $_ }).Count -eq 0
        } | ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous artifact $name" }
        $externs += @('--extern', "$name=$($files[0])")
    }
    foreach ($linkPath in @($records | Where-Object { $_.reason -eq 'build-script-executed' } | ForEach-Object { $_.linked_paths } | Select-Object -Unique)) { $externs += @('-L', $linkPath) }
    $root = (Join-Path $repo 'apps/lab-workbench/src').Replace('\', '/')
    $modules = @('client','dispatcher','external','gui','readonly_limits','readonly_values','readonly_cache','model','ownership','parent','presentation','private_ipc','recovery','storage','supervisor') | ForEach-Object {
        $path = if (Test-Path "$root/$_.rs") { "$root/$_.rs" } else { "$root/$_/mod.rs" }
        "#[path = `"$path`"] mod $_;"
    }
    $directory = Join-Path $repo "target/private-ipc/$BuildProfile"
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $source = Join-Path $directory 'fixture.rs'
    $wrapper = @"
#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
$($modules -join "`n")
#[path = "$root/gui/rebuild.rs"] mod rebuild;
fn main() { if let Err(error) = private_ipc::fixture::run() { eprintln!("{error:?}"); std::process::exit(91); } }
"@
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $target = if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
    $flags = if ($BuildProfile -eq 'dev') { @('-C','opt-level=0','-C','debug-assertions=yes','-C','overflow-checks=yes') } else { @('-C','opt-level=3','-C','debug-assertions=no','-C','overflow-checks=no') }
    $fixture = Join-Path $directory 'private-ipc-fixture.exe'
    & rustc --edition=2024 $source @externs @flags -D warnings -L "dependency=$repo/target/$target/deps" -o $fixture
    if ($LASTEXITCODE -ne 0) { throw 'IPC fixture compile failed' }
    Write-Output "FIXTURE=$fixture"
    if (-not $FixtureOnly) {
        $previous = $env:LAB_PRIVATE_IPC_FIXTURE
        try {
            $env:LAB_PRIVATE_IPC_FIXTURE = $fixture
            & cargo test -p lab-workbench --locked --profile $BuildProfile -- private_ipc::native_tests --ignored --test-threads=1 --nocapture
            if ($LASTEXITCODE -ne 0) { throw 'Native IPC qualification failed' }
        } finally { $env:LAB_PRIVATE_IPC_FIXTURE = $previous }
    }
} finally { Pop-Location }
