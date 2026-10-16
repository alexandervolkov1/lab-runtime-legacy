# Native lifecycle probe linked against the real Workbench executable's artifacts.
param(
    [ValidateSet('dev', 'release')][string]$BuildProfile = 'dev',
    [switch]$DiagnosticProvenance,
    [switch]$ProjectionOnly,
    [switch]$PlotOnly,
    [switch]$GeometryOnly,
    [switch]$MemoryOnly,
    [switch]$FramesOnly
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    $version = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or $version -notcontains 'host: x86_64-pc-windows-msvc') {
        throw 'Renderer qualification requires native x86_64 Windows/MSVC'
    }
    if ($DiagnosticProvenance -and $BuildProfile -ne 'dev') { throw 'Diagnostic baseline is dev-only' }
    if ($DiagnosticProvenance -and $ProjectionOnly) { throw 'Projection qualification requires the fixed profiles' }
    if ($DiagnosticProvenance -and $PlotOnly) { throw 'Plot qualification requires the fixed profiles' }
    if ($DiagnosticProvenance -and $GeometryOnly) { throw 'Geometry qualification requires the fixed profiles' }
    if ($DiagnosticProvenance -and $MemoryOnly) { throw 'Memory qualification requires the fixed profiles' }
    if ($DiagnosticProvenance -and $FramesOnly) { throw 'Frame qualification requires the fixed profiles' }
    $cargoArgs = @('build', '-p', 'lab-workbench', '--locked', '--profile', $BuildProfile, '--message-format=json')
    if ($DiagnosticProvenance) {
        # Reproduce the reviewed pre-fix dependency cfg, without editing Cargo.toml.
        $cargoArgs += @('--config', 'profile.dev.package."egui:0.36.2".debug-assertions=true')
    }
    $artifacts = & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Workbench executable build failed' }
    $records = $artifacts | ForEach-Object { $_ | ConvertFrom-Json }
    $compiled = @($records | Where-Object { $_.reason -eq 'compiler-artifact' })
    $application = @($compiled | Where-Object { $_.target.name -eq 'lab-workbench' })
    $egui = @($compiled | Where-Object { $_.target.name -eq 'egui' })
    if ($application.Count -ne 1 -or $egui.Count -ne 1) { throw 'Ambiguous executable/egui artifact' }
    $appDebug = $BuildProfile -eq 'dev'
    if ($application[0].profile.debug_assertions -ne $appDebug -or
        $application[0].profile.overflow_checks -ne $appDebug -or
        $egui[0].profile.debug_assertions -ne $DiagnosticProvenance.IsPresent -or
        $egui[0].profile.overflow_checks -ne $appDebug) {
        throw 'Actual Cargo artifact profiles differ from the qualified configuration'
    }
    # All other dev crates keep Cargo's ordinary assertion/overflow settings.
    if ($appDebug -and @($compiled | Where-Object {
        $_.target.name -ne 'egui' -and (-not $_.profile.debug_assertions -or -not $_.profile.overflow_checks)
    }).Count -ne 0) { throw 'A non-egui dev crate lost its checks' }

    $metadataText = & cargo metadata --no-deps --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Metadata failed' }
    $metadata = $metadataText | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq 'lab-workbench' }
    $externs = @()
    foreach ($dependency in $package.dependencies | Where-Object { $null -eq $_.kind }) {
        $name = $dependency.name.Replace('-', '_')
        $files = @($compiled | Where-Object {
            $artifact = $_
            $artifact.target.name -eq $name -and @($dependency.features | Where-Object {
                $artifact.features -notcontains $_
            }).Count -eq 0
        } |
            ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous exact compiled artifact for $name" }
        $externs += @('--extern', "$name=$($files[0])")
    }
    foreach ($linkPath in @($records | Where-Object { $_.reason -eq 'build-script-executed' } |
        ForEach-Object { $_.linked_paths } | Select-Object -Unique)) {
        $externs += @('-L', $linkPath)
    }
    # Test-only attribution of the first Context's 24-byte singleton baseline.
    # Link the existing transitive crate; do not add a dependency or feature.
    $hashFiles = @($compiled | Where-Object { $_.target.name -eq 'ahash' } |
        ForEach-Object { $_.filenames } | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
    if ($hashFiles.Count -ne 1) { throw 'Ambiguous ahash artifact' }
    $externs += @('--extern', "ahash=$($hashFiles[0])")
    $variant = if ($DiagnosticProvenance) { 'dev-provenance' } else { $BuildProfile }
    $probeDir = Join-Path $repo "target/v2-renderer-allocation-probe/$variant"
    New-Item -ItemType Directory -Force -Path $probeDir | Out-Null
    $sourceRoot = (Join-Path $repo 'apps/lab-workbench/src').Replace('\', '/')
    $support = (Join-Path $repo 'scripts/support').Replace('\', '/')
    $rootModules = @()
    foreach ($name in @('client', 'dispatcher', 'external', 'readonly_limits','readonly_values', 'readonly_cache', 'model', 'ownership', 'parent', 'presentation', 'recovery', 'storage','time_context', 'supervisor', 'private_ipc')) {
        if ($name -eq 'supervisor') {
            $path = (Join-Path $repo 'apps/lab-workbench/src/supervisor/mod.rs').Replace('\','/')
            $rootModules += "#[path = `"$path`"] mod supervisor;"
            continue
        }
        $path = "$sourceRoot/$name.rs"
        if (-not (Test-Path -LiteralPath $path)) { $path = "$sourceRoot/$name/mod.rs" }
        if (-not (Test-Path -LiteralPath $path)) { throw "Missing production module $name" }
        $rootModules += "#[path = `"$path`"] mod $name;"
    }
    # Actual v1/v2 production modules. This bridge starts no GUI app, client,
    # worker, filesystem operation or Runtime connection; it only runs headless Ui.
    $wrapper = @"
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
$($rootModules -join "`n")
#[path = "$sourceRoot/gui/rebuild.rs"] mod rebuild;
mod gui {
pub(crate) use crate::rebuild;
#[path = "$sourceRoot/gui/app.rs"] mod app;
#[path = "$sourceRoot/gui/controls.rs"] mod controls;
#[path = "$sourceRoot/gui/plot_ui.rs"] mod plot_ui;
#[path = "$sourceRoot/gui/presentation.rs"] mod presentation;
#[path = "$sourceRoot/gui/readonly_v2/mod.rs"] pub(crate) mod readonly_v2;
#[path = "$support/v1-renderer-probe.rs"] mod v1_probe;
pub(crate) use v1_probe::V1Scene;
}
use gui::readonly_v2;
#[path = "$sourceRoot/gui/readonly_v2/fixtures.rs"] mod fixtures;
#[path = "$sourceRoot/gui/readonly_v2/plot_fixtures.rs"] mod plot_fixtures;
#[path = "$sourceRoot/gui/readonly_v2/plot_geometry_fixtures.rs"] mod plot_geometry_fixtures;
#[path = "$support/v2-renderer-allocation-probe.rs"] mod probe;
#[path = "$support/glow-seam-check.rs"] mod glow_seam_check;
fn main() { probe::run(); }
"@
    $source = Join-Path $probeDir 'main.rs'
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $binary = Join-Path $probeDir 'v2-renderer-allocation-probe.exe'
    $directory = if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
    $rustFlags = if ($appDebug) { @('-C', 'debug-assertions=yes', '-C', 'overflow-checks=yes') } else {
        @('-C', 'opt-level=3', '-C', 'debug-assertions=no', '-C', 'overflow-checks=no')
    }
    & rustc --edition=2024 -D warnings $source @externs @rustFlags -L "dependency=$(Join-Path $repo "target/$directory/deps")" -o $binary
    if ($LASTEXITCODE -ne 0) { throw 'Renderer probe compilation failed' }
    Write-Output 'Public Glow API and borrowed native App lifetime: compile PASS (no GPU execution)'
    # Compile the same real modules with deliberately invalid ownership examples.
    # This verifies borrowing enforcement without adding a test-only dependency.
    $borrowCases = @(
        @('next-frame', 'E0499', 'fn denied(p: &mut readonly_v2::Preview<''_>) { let f = p.run(&model::RuntimeObservations::default(), Default::default()).unwrap(); let _ = p.run(&model::RuntimeObservations::default(), Default::default()); f.discard(); }'),
        @('retire-context', 'E0499', 'fn denied(p: &mut readonly_v2::Preview<''_>) { let f = p.run(&model::RuntimeObservations::default(), Default::default()).unwrap(); p.retire_context(); f.discard(); }'),
        @('drop-preview', 'E0505', 'fn denied(mut p: readonly_v2::Preview<''_>) { let f = p.run(&model::RuntimeObservations::default(), Default::default()).unwrap(); drop(p); f.discard(); }'),
        @('second-host-preview', 'E0499', 'fn denied(d: &presentation::v2::AdmittedV2Document, s: &mut readonly_v2::host_session::HostSession<readonly_v2::texture_owner::CpuOnly>) { let mut a = readonly_v2::Preview::with_session(d, readonly_v2::PresentationEpoch { owner: [0;16], revision: 0 }, s); let _b = readonly_v2::Preview::with_session(d, readonly_v2::PresentationEpoch { owner: [0;16], revision: 0 }, s); a.retire_context(); }'),
        @('drop-host-session', 'E0505', 'fn denied(d: &presentation::v2::AdmittedV2Document, mut s: readonly_v2::host_session::HostSession<readonly_v2::texture_owner::CpuOnly>) { let mut p = readonly_v2::Preview::with_session(d, readonly_v2::PresentationEpoch { owner: [0;16], revision: 0 }, &mut s); drop(s); p.retire_context(); }')
    )
    $borrowCases += @(
        @('handoff-live-document', 'E0499', 'fn denied<B: readonly_v2::texture_owner::FrameBackend>(o: &mut private_ipc::DocumentOwner<''_, B>) { let (d,h,s) = o.parts().unwrap(); let mut p = readonly_v2::Preview::with_session(d, readonly_v2::PresentationEpoch { owner:h.epoch.owner, revision:h.epoch.revision }, s); let _ = o.release_retired(); p.retire_context(); }'),
        @('handoff-admit-live-preview', 'E0499', 'fn denied<B: readonly_v2::texture_owner::FrameBackend>(r: &mut private_ipc::Replica, o: &mut private_ipc::DocumentOwner<''_, B>) { let (d,h,s) = o.parts().unwrap(); let mut p = readonly_v2::Preview::with_session(d, readonly_v2::PresentationEpoch { owner:h.epoch.owner, revision:h.epoch.revision }, s); let _ = r.admit_renderer_document(o); p.retire_context(); }'),
        @('handoff-borrowed-assembly', 'E0502', 'fn denied(r: &mut private_ipc::Replica) { let bytes = r.candidate_bytes().unwrap(); r.stop(); std::hint::black_box(bytes); }'),
        @('handoff-original-session', 'E0505', 'fn denied() { let mut s = readonly_v2::host_session::HostSession::new(readonly_v2::texture_owner::CpuOnly); let (_r,o) = private_ipc::Replica::renderer(&mut s); drop(s); std::hint::black_box(o.document()); }')
    )
    $borrowCases += ,@('selected-view-credit', 'E0499', 'fn denied(r: &mut private_ipc::Replica) { use readonly_v2::observation_source::ReadOnlyProvider; let v = r.selected_provider(std::time::Instant::now()).unwrap().unwrap(); r.stop(); std::hint::black_box(v.current()); }')
    foreach ($case in $borrowCases) {
        $deniedSource = Join-Path $probeDir "denied-$($case[0]).rs"
        [IO.File]::WriteAllText($deniedSource, $wrapper + "`n" + $case[2], [Text.UTF8Encoding]::new($false))
        $diagnostic = & rustc --edition=2024 --emit=metadata $deniedSource @externs @rustFlags -L "dependency=$(Join-Path $repo "target/$directory/deps")" -o (Join-Path $probeDir 'denied.rmeta') 2>&1
        if ($LASTEXITCODE -eq 0 -or ($diagnostic -join "`n") -notmatch "error\[$($case[1])\]") {
            throw "Ownership compile-fail gate did not reject $($case[0]) as expected: $diagnostic"
        }
        $diagnostic | Set-Content -LiteralPath (Join-Path $probeDir "denied-$($case[0]).txt")
        Write-Output "Ownership compile-fail PASS: $($case[0]) ($($case[1]))"
    }
    $evidence = Join-Path $probeDir 'artifacts.json'
    $compiled | Where-Object { $_.target.name -in @('egui', 'eframe', 'lab-workbench') } |
        ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $evidence
    $provenance = if ($DiagnosticProvenance) { 'on' } else { 'off' }
    $scenarios = @('hash-baseline', 'empty', 'ids-numeric', 'ids-object', 'small', 'v1', 'maximum')
    if (-not $DiagnosticProvenance) { $scenarios += @('bounded', 'projection', 'plot', 'geometry', 'memory', 'frames', 'host-no-upload', 'retirement') }
    if ($ProjectionOnly) { $scenarios = @('projection') }
    if ($PlotOnly) { $scenarios = @('plot') }
    if ($GeometryOnly) { $scenarios = @('geometry') }
    if ($MemoryOnly) { $scenarios = @('memory') }
    if ($FramesOnly) { $scenarios = @('frames', 'host-no-upload', 'retirement') }
    foreach ($scenario in $scenarios) {
        $output = & $binary $scenario $provenance
        $result = $LASTEXITCODE
        $output | Set-Content -LiteralPath (Join-Path $probeDir "$scenario.txt")
        $output
        if ($scenario -eq 'geometry' -and $result -eq 2) {
            throw "CPU Plot geometry qualification BLOCKED: see $probeDir/geometry.txt (exit 2)"
        }
        if ($result -ne 0) { throw "Renderer probe failed: $variant/$scenario (exit $result)" }
    }
} finally {
    Pop-Location
}
