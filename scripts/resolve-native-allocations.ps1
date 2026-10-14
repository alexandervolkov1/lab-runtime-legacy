# Resolves bounded allocator stack RVAs using the exact MSVC launcher link map.
# A symbol's nearest preceding map entry identifies a function interval, not an
# allocation's final owner or proof that retention is harmless or a leak.
param([Parameter(Mandatory)][string]$Map, [Parameter(Mandatory)][string]$Transcript)
$ErrorActionPreference = 'Stop'
$mapText = Get-Content -LiteralPath $Map
$preferred = $mapText | Select-String 'Preferred load address is\s+([0-9A-Fa-f]+)' | Select-Object -First 1
if ($null -eq $preferred) { throw 'Missing link map image base' }
$imageBase = [Convert]::ToUInt64($preferred.Matches[0].Groups[1].Value, 16)
$symbols = @($mapText | ForEach-Object {
    if ($_ -match '^\s+[0-9a-fA-F]+:[0-9a-fA-F]+\s+(\S+)\s+([0-9a-fA-F]{16})\s') {
        [pscustomobject]@{ Rva=([Convert]::ToUInt64($Matches[2],16) - $imageBase); Name=$Matches[1] }
    }
} | Sort-Object Rva)
if ($symbols.Count -eq 0) { throw 'No link map symbols' }
Get-Content -LiteralPath $Transcript | ForEach-Object {
    if ($_ -match '^RUST_RESIDUAL_ATTRIBUTION|^RESIDUAL_BLOCK') { $_ }
    elseif ($_ -match 'residual_rva=0x([0-9a-fA-F]+)') {
        $rva = [Convert]::ToUInt64($Matches[1],16)
        $low = 0; $high = $symbols.Count - 1; $found = -1
        while ($low -le $high) {
            $middle = [int][Math]::Floor(($low + $high) / 2)
            if ($symbols[$middle].Rva -le $rva) { $found = $middle; $low = $middle + 1 } else { $high = $middle - 1 }
        }
        if ($found -ge 0 -and $rva -le $symbols[-1].Rva) { "  $($symbols[$found].Name) +$($rva - $symbols[$found].Rva)" }
        else { "  unresolved_rva=$rva" }
    }
}
