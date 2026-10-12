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
    # Pull constants verbatim from their owners; do not maintain a copied profile.
    $external = Get-Content 'apps/lab-workbench/src/external.rs' | Where-Object {
        $_ -match '^pub\(crate\) const (JSON_BODY_BYTES|FRAME_BYTES|JSON_DEPTH|JSON_VALUES|JSON_STRING_BYTES|NAME_BYTES):'
    }
    if ($external.Count -ne 6) { throw 'Unexpected wire constant declarations' }
    $probeDir = Join-Path $repo 'target/v2-allocation-probe'
    New-Item -ItemType Directory -Force -Path $probeDir | Out-Null
    $module = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/mod.rs').Replace('\', '/')
    $document = (Join-Path $repo 'apps/lab-workbench/src/presentation/document.rs').Replace('\', '/')
    $reference = (Join-Path $repo 'apps/lab-workbench/src/presentation/reference.rs').Replace('\', '/')
    $fixture = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/admission_fixtures.rs').Replace('\', '/')
    $conversionFixture = (Join-Path $repo 'apps/lab-workbench/src/presentation/v2/conversion_fixtures.rs').Replace('\', '/')
    $probe = (Join-Path $repo 'scripts/support/v2-allocation-probe.rs').Replace('\', '/')
    $wrapper = @"
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
mod external {
$($external -join "`n")
}
mod presentation {
#[path = "$document"] mod document;
#[path = "$reference"] mod reference;
pub(crate) use document::*;
pub(crate) use reference::*;
}
#[path = "$module"] mod v2;
#[path = "$fixture"] mod fixtures;
#[path = "$conversionFixture"] mod conversion_fixtures;
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
