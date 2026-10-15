# Compile the inert Cargo target and reject moving its exact native Device to
# another thread. No native window, event loop, GL call or GUI child is executed.
param([ValidateSet('dev','release')][string]$BuildProfile = 'dev')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    if ((& rustc -vV) -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Windows/MSVC required' }
    $directory = Join-Path $repo "target/native-child-capsule/$BuildProfile"
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $records = @(& cargo build -p lab-native-child --locked --profile $BuildProfile --message-format=json | ForEach-Object { $_ | ConvertFrom-Json })
    $buildExit = $LASTEXITCODE
    $records | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $directory 'artifacts.json')
    if ($buildExit -ne 0) {
        $records | Where-Object reason -eq 'compiler-message' | ForEach-Object { Write-Output $_.message.rendered }
        throw 'Native capsule Cargo build failed'
    }
    & cargo clippy -p lab-native-child --all-targets --locked --profile $BuildProfile -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Native capsule Clippy failed' }
    $compiled = @($records | Where-Object reason -eq 'compiler-artifact')
    $externs = @()
    foreach ($name in @('eframe','glutin','glutin_winit','winit','raw_window_handle','egui_plot','serde','serde_json','lab_windows_job')) {
        $files = @($compiled | Where-Object { $_.target.name -eq $name } | ForEach-Object filenames | Where-Object { $_ -like '*.rlib' } | Select-Object -Unique)
        if ($files.Count -ne 1) { throw "Ambiguous native artifact $name" }
        $externs += @('--extern', "$name=$($files[0])")
    }
    foreach ($path in @($records | Where-Object reason -eq 'build-script-executed' | ForEach-Object linked_paths | Select-Object -Unique)) { $externs += @('-L', $path) }
    $native = (Join-Path $repo 'scripts/support/native-glow-device.rs').Replace('\','/')
    $types = (Join-Path $repo 'apps/lab-native-child/src/consumer.rs').Replace('\','/')
    $source = Join-Path $directory 'denied-send.rs'
    $wrapper = @"
//! Compile-fail proof for the real native Device's non-Send boundary.
#![deny(unsafe_code)]
#![allow(dead_code, unused_imports)]
#[path = "$types"] mod consumer;
pub(crate) use consumer::{presentation, private_ipc, readonly_limits, readonly_v2, readonly_values};
mod gui { pub(crate) use crate::readonly_v2; }
#[path = "$native"] mod device;
fn main() { fn send<T: Send>() {} send::<device::Device>(); }
"@
    [IO.File]::WriteAllText($source, $wrapper, [Text.UTF8Encoding]::new($false))
    $target = if ($BuildProfile -eq 'dev') { 'debug' } else { 'release' }
    $flags = if ($BuildProfile -eq 'dev') { @('-C','debug-assertions=yes','-C','overflow-checks=yes') } else { @('-C','opt-level=3','-C','debug-assertions=no','-C','overflow-checks=no') }
    $diagnostics = & rustc --edition=2024 -D warnings --emit=metadata $source @externs @flags -L "dependency=$repo/target/$target/deps" -o (Join-Path $directory 'denied-send.rmeta') 2>&1
    if ($LASTEXITCODE -eq 0 -or ($diagnostics -join "`n") -notmatch 'error\[E0277\]') { throw "Device Send proof failed: $diagnostics" }
    $diagnostics | Set-Content -LiteralPath (Join-Path $directory 'denied-send.txt')
    $forbidden = Join-Path $directory 'denied-forbid.rs'
    $protected = $wrapper.Replace('#![deny(unsafe_code)]','#![forbid(unsafe_code)]').Replace('fn main() { fn send<T: Send>() {} send::<device::Device>(); }','fn main() {}')
    [IO.File]::WriteAllText($forbidden, $protected, [Text.UTF8Encoding]::new($false))
    $diagnostics = & rustc --edition=2024 -D warnings --emit=metadata $forbidden @externs @flags -L "dependency=$repo/target/$target/deps" -o (Join-Path $directory 'denied-forbid.rmeta') 2>&1
    if ($LASTEXITCODE -eq 0 -or ($diagnostics -join "`n") -notmatch 'error\[E0453\]') { throw "Protected policy proof failed: $diagnostics" }
    $diagnostics | Set-Content -LiteralPath (Join-Path $directory 'denied-forbid.txt')
    $binary = @($compiled | Where-Object { $_.target.name -eq 'lab-native-child' } | ForEach-Object executable | Where-Object { $_ })
    if ($binary.Count -ne 1) { throw 'Ambiguous Cargo target binary' }
    Get-FileHash -Algorithm SHA256 -LiteralPath $binary[0]
    Write-Output 'NATIVE CAPSULE Cargo build/Clippy PASS; non-Send E0277 and protected-policy E0453 rejection PASS; no GPU execution'
    # Both negative compiler controls intentionally return nonzero. Do not expose
    # the last expected rejection as the outcome of this successful script.
    $global:LASTEXITCODE = 0
} finally { Pop-Location }
