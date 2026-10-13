# Native egui heap measurements using exact locked artifacts and production code.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    $version = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $version -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'Renderer qualification requires native x86_64 Windows/MSVC'
    }
    $artifacts = & cargo test -p lab-workbench --locked --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Probe dependency build failed' }
    $records = $artifacts | ForEach-Object { $_ | ConvertFrom-Json }
    $externs = @()
    foreach ($name in @('serde', 'serde_json', 'eframe')) {
        $files = @($records | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq $name } |
            ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous exact compiled artifact for $name" }
        $externs += '--extern'
        $externs += "$name=$($files[0])"
    }
    # Preserve native search paths from these exact Cargo build-script records,
    # including windows-targets import libraries used transitively by eframe.
    foreach ($linkPath in @($records | Where-Object { $_.reason -eq 'build-script-executed' } |
        ForEach-Object { $_.linked_paths } | Select-Object -Unique)) {
        $externs += '-L'
        $externs += $linkPath
    }
    $external = Get-Content 'apps/lab-workbench/src/external.rs' | Where-Object {
        $_ -match '^pub\(crate\) const (JSON_BODY_BYTES|FRAME_BYTES|JSON_DEPTH|JSON_VALUES|JSON_STRING_BYTES|NAME_BYTES):'
    }
    if ($external.Count -ne 6) { throw 'Unexpected wire constant declarations' }
    $probeDir = Join-Path $repo 'target/v2-renderer-allocation-probe'
    New-Item -ItemType Directory -Force -Path $probeDir | Out-Null
    $sourceRoot = (Join-Path $repo 'apps/lab-workbench/src').Replace('\', '/')
    $probe = (Join-Path $repo 'scripts/support/v2-renderer-allocation-probe.rs').Replace('\', '/')
    $wrapper = @"
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
mod external {
$($external -join "`n")
}
mod presentation {
#[path = "$sourceRoot/presentation/document.rs"] mod document;
#[path = "$sourceRoot/presentation/reference.rs"] mod reference;
#[path = "$sourceRoot/presentation/v2/mod.rs"] pub(crate) mod v2;
pub(crate) use document::*;
pub(crate) use reference::*;
}
mod model {
#[path = "$sourceRoot/model/projections.rs"] mod projections;
#[path = "$sourceRoot/model/read_binding.rs"] mod read_binding;
pub(crate) use projections::{Freshness, RuntimeObservations};
pub(crate) use read_binding::*;
}
#[path = "$sourceRoot/gui/readonly_v2/mod.rs"] mod readonly_v2;
#[path = "$sourceRoot/gui/readonly_v2/fixtures.rs"] mod fixtures;
#[path = "$probe"] mod probe;
fn main() { probe::run(); }
"@
    $source = Join-Path $probeDir 'main.rs'
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $binary = Join-Path $probeDir 'v2-renderer-allocation-probe.exe'
    & rustc --edition=2024 -D warnings $source @externs -L "dependency=$(Join-Path $repo 'target/debug/deps')" -o $binary
    if ($LASTEXITCODE -ne 0) { throw 'Renderer probe compilation failed' }
    foreach ($mode in @('context', 'debug-epochs')) {
        & $binary $mode
        if ($LASTEXITCODE -ne 0) { throw "Renderer probe failed: $mode" }
    }
} finally {
    Pop-Location
}
