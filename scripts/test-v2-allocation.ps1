# Isolated native allocation probe; no dependency/lint changes to the workspace.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    $version = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $version -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'This allocation qualification requires native x86_64 Windows/MSVC'
    }
    $artifacts = & cargo test -p lab-workbench --locked --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Probe dependency build failed' }
    $records = $artifacts | ForEach-Object { $_ | ConvertFrom-Json }
    $externs = @()
    foreach ($name in @('serde', 'serde_json')) {
        $files = @($records | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq $name } |
            ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous exact compiled artifact for $name" }
        $externs += '--extern'
        $externs += "$name=$($files[0])"
    }
    # Include the same canonical safe limit declarations; no copied quota profile.
    $limits = (Join-Path $repo 'apps/lab-workbench/src/readonly_limits.rs').Replace('\', '/')
    $probeDir = Join-Path $repo 'target/v2-allocation-probe'
    New-Item -ItemType Directory -Force -Path $probeDir | Out-Null
    $module = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/mod.rs').Replace('\', '/')
    $document = (Join-Path $repo 'apps/lab-workbench/src/presentation/document.rs').Replace('\', '/')
    $reference = (Join-Path $repo 'apps/lab-workbench/src/presentation/reference.rs').Replace('\', '/')
    $fixture = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/admission_fixtures.rs').Replace('\', '/')
    $conversionFixture = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/conversion_fixtures.rs').Replace('\', '/')
    $exportFixture = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/export_fixtures.rs').Replace('\', '/')
    $probe = (Join-Path $repo 'scripts/support/v2-allocation-probe.rs').Replace('\', '/')
    $wrapper = @"
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
#[path = "$limits"] mod readonly_limits;
mod presentation {
#[path = "$document"] mod document;
#[path = "$reference"] mod reference;
pub(crate) use document::*;
pub(crate) use reference::*;
}
#[path = "$module"] mod v2;
#[path = "$fixture"] mod fixtures;
#[path = "$conversionFixture"] mod conversion_fixtures;
#[path = "$exportFixture"] mod export_fixtures;
#[path = "$probe"] mod probe;
fn main() { probe::run(); }
"@
    $source = Join-Path $probeDir 'main.rs'
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $binary = Join-Path $probeDir 'v2-allocation-probe.exe'
    & rustc --edition=2024 -D warnings $source @externs -L "dependency=$(Join-Path $repo 'target/debug/deps')" -o $binary
    if ($LASTEXITCODE -ne 0) { throw 'Probe compilation failed' }
    & $binary
    if ($LASTEXITCODE -ne 0) { throw 'Probe failed' }
} finally {
    Pop-Location
}
