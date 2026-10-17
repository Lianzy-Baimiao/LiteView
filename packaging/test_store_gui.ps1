param([Parameter(Mandatory = $true)][string]$Output)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'Packaged GUI replay is only allowed on a disposable GitHub-hosted Windows runner.'
}
$Output = [IO.Path]::GetFullPath($Output)
$prefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $Output.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $Output)) {
    throw 'GUI reports must use a new directory inside RUNNER_TEMP.'
}
New-Item -ItemType Directory -Path $Output | Out-Null
$family = 'ice.LiteView_rz8m2sa64m4em'
$aumid = "$family!LiteView"
$package = Get-AppxPackage -Name ice.LiteView | Where-Object PackageFamilyName -eq $family
if (-not $package -or $package.Version -ne '1.0.0.0') { throw 'Expected the installed 1.0.0.0 Store package.' }
$binary = Join-Path $package.InstallLocation 'LiteView.exe'
if ((Get-FileHash -LiteralPath $binary).Hash -ne '21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a') {
    throw 'Installed payload does not match the released executable.'
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IStoreGuiActivationManager {
    [PreserveSig] int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string id,
        [MarshalAs(UnmanagedType.LPWStr)] string args, uint options, out uint processId);
}
[ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")]
class StoreGuiActivationManager {}
public static class StoreGuiActivation {
    public static uint Run(string id, string args) {
        var manager = (IStoreGuiActivationManager)new StoreGuiActivationManager();
        try {
            uint pid;
            Marshal.ThrowExceptionForHR(manager.ActivateApplication(id, args, 0, out pid));
            return pid;
        } finally { Marshal.ReleaseComObject(manager); }
    }
}
'@
$diagnostics = Join-Path $Output 'real-aumid-diagnostics.json'
$processId = [StoreGuiActivation]::Run($aumid, ('--diagnostics "' + $diagnostics + '"'))
$process = [Diagnostics.Process]::GetProcessById($processId)
try {
    $null = $process.Handle
    if (-not $process.WaitForExit(30000)) { throw 'Real AUMID diagnostic activation timed out.' }
    if ($null -eq $process.ExitCode -or $process.ExitCode -ne 0) { throw 'Real AUMID diagnostic activation failed.' }
} finally { $process.Dispose() }
$actual = Get-Content -LiteralPath $diagnostics -Raw | ConvertFrom-Json
if (-not $actual.packaged -or $actual.package_family -ne $family -or $actual.application_id -ne $aumid -or
    -not $actual.package_local_folder -or $actual.config_dir -ne $actual.package_local_folder) {
    throw 'Real AUMID activation has an incorrect identity or configuration directory.'
}
$python = (Get-Command python.exe -ErrorAction Stop).Source
$script = Join-Path $PSScriptRoot 'test_store_gui.py'
$result = Join-Path $Output 'gui-result.json'
$captures = Join-Path $Output 'captures'
$arguments = '"{0}" --binary "{1}" --output "{2}" --result "{3}"' -f $script, $binary, $captures, $result
# The child bootstrap sets the app's replay environment itself. This does not
# alter user/machine environment or enable debugging/Developer Mode for WACK.
Invoke-CommandInDesktopPackage -PackageFamilyName $family -AppId LiteView -Command $python -Args $arguments -PreventBreakaway
$deadline = (Get-Date).AddSeconds(240)
while (-not (Test-Path -LiteralPath $result) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 1 }
if (-not (Test-Path -LiteralPath $result)) { throw 'Packaged GUI bootstrap did not produce a result within 240 seconds.' }
$gui = Get-Content -LiteralPath $result -Raw | ConvertFrom-Json
if ($gui.status -ne 'pass') { throw "Packaged GUI replay failed: $($gui.error)" }
if ($gui.scenes.Count -ne 4) { throw 'Expected all four GUI scenes.' }
Write-Host 'PASS: separate real AUMID diagnostics plus four installed-package identity GUI replays.'
