param([Parameter(Mandatory = $true)][string]$ResultPath)

# Invoke-CommandInDesktopPackage does not propagate its child's exit code.
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $ResultPath -PathType Leaf)) {
    throw "No probe result (missing or timeout): $ResultPath"
}
$raw = Get-Content -LiteralPath $ResultPath -Raw -Encoding UTF8
if (-not $raw -or -not $raw.TrimStart().StartsWith('{')) {
    throw "Probe result is not a JSON object: $ResultPath"
}
$result = $raw | ConvertFrom-Json
if ($null -eq $result -or $result.status -isnot [string] -or $result.status -cne 'pass') {
    throw "Probe did not pass; inspect $ResultPath"
}
Write-Output $raw
