# Safe local regression: synthetic processes only; no installer or registry access.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This process-tree regression requires Windows.'
}
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $PSScriptRoot 'test_windows_installer.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
# Load only these function definitions, never the guarded registry/installer script body.
foreach ($name in @('Assert-True', 'Invoke-Checked')) {
    $definition = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
    }, $true)
    if (-not $definition) { throw "Missing helper: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('liteview-process-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
$started = Join-Path $root 'started.txt'
$finished = Join-Path $root 'finished.txt'
$shell = (Get-Process -Id $PID).Path
$quote = { param($s) "'" + $s.Replace("'", "''") + "'" }
$child = "[IO.File]::WriteAllText($(& $quote $started), 'started'); Start-Sleep -Milliseconds 2000; [IO.File]::WriteAllText($(& $quote $finished), 'finished')"
$child64 = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($child))
$parent = @"
Start-Process -FilePath $(& $quote $shell) -ArgumentList '-NoProfile -NonInteractive -EncodedCommand $child64' -WindowStyle Hidden | Out-Null
`$deadline = [DateTime]::UtcNow.AddSeconds(10)
while (-not (Test-Path -LiteralPath $(& $quote $started))) {
    if ([DateTime]::UtcNow -gt `$deadline) { exit 2 }
    Start-Sleep -Milliseconds 20
}
"@
$parent64 = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($parent))
try {
    Invoke-Checked $shell @('-NoProfile', '-NonInteractive', '-EncodedCommand', $parent64)
    Assert-True (Test-Path -LiteralPath $finished) 'Returned before the child cleanup process completed.'
    Write-Host 'PASS: installer process helper waits for descendants, not just the launcher.'
    $failure = $null
    try { Invoke-Checked $shell @('-NoProfile', '-NonInteractive', '-Command', 'exit 7') }
    catch { $failure = $_.Exception.Message }
    Assert-True ($null -ne $failure -and $failure -like 'Process exited 7:*') 'Nonzero exit status was not propagated.'
    Write-Host 'PASS: nonzero launcher exit status fails the check.'
}
finally {
    # On the intentionally failing old implementation, let the bounded child finish first.
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $finished) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    # Delete only the two known marker files; never recursively remove an inferred path.
    foreach ($file in @($started, $finished)) {
        if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file }
    }
    Remove-Item -LiteralPath $root
}
