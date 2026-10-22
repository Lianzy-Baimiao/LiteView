# Pure receipt fixtures; never activates or installs a package.
$ErrorActionPreference = 'Stop'
$checker = Join-Path $PSScriptRoot 'check_windows_probe_result.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) ('liteview-probe-result-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
$path = Join-Path $root 'result.json'
try {
    foreach ($value in @($null, '', 'invalid json', '{}', 'null', '[{"status":"pass"}]', '{"status":["pass"]}', '{"status":"fail"}', '{"status":"error"}')) {
        if ($null -ne $value) { [IO.File]::WriteAllText($path, $value) }
        $failed = $false
        try { & $checker -ResultPath $path | Out-Null } catch { $failed = $true }
        if (-not $failed) { throw "Invalid result was accepted: $value" }
    }
    [IO.File]::WriteAllText($path, '{"status":"pass"}')
    & $checker -ResultPath $path | Out-Null
    Write-Host 'PASS: missing, malformed and failed receipts fail; only explicit pass succeeds.'
} finally {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    Remove-Item -LiteralPath $root
}
