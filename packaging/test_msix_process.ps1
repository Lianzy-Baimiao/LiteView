# Synthetic activation only: no application, package, registry or certificate changes.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $PSScriptRoot 'test_msix_lifecycle.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
foreach ($name in @('Assert-True', 'Invoke-Packaged')) {
    $definition = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $true)
    if (-not $definition) { throw "Missing helper: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
public static class LiteViewActivation {
    public static uint Run(string id, string args) {
        var info = new ProcessStartInfo(id, args);
        info.UseShellExecute = false;
        info.CreateNoWindow = true;
        info.WindowStyle = ProcessWindowStyle.Hidden;
        using (var process = Process.Start(info)) { return (uint)process.Id; }
    }
}
'@
$aumid = (Get-Process -Id $PID).Path
Invoke-Packaged '-NoProfile -NonInteractive -Command "Start-Sleep -Milliseconds 500; exit 0"'
Write-Host 'PASS: successful activated process retains exit code zero.'
$failure = $null
try { Invoke-Packaged '-NoProfile -NonInteractive -Command "Start-Sleep -Milliseconds 500; exit 7"' }
catch { $failure = $_.Exception.Message }
Assert-True ($failure -and $failure -match 'exit[ =]7\b') 'Nonzero activated process status was not reported.'
Write-Host 'PASS: nonzero activated process status fails with the actual code.'
