param(
    [Parameter(Mandatory = $true)][string]$Package,
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$MakeAppx,
    [Parameter(Mandatory = $true)][string]$SignTool
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'This test changes package registration and certificate trust. Run only on a disposable GitHub-hosted Windows runner.'
}
$Package = (Resolve-Path -LiteralPath $Package).Path
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$root = Join-Path $env:RUNNER_TEMP ('liteview-msix-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$name = 'ice.LiteView'
$publisher = 'CN=56079E3F-5861-4D9C-B9C3-93F76224C42E'
$family = 'ice.LiteView_rz8m2sa64m4em'
$aumid = "$family!LiteView"
$classicKey = 'HKCU:\Software\Classes\LiteView.png\shell\open\command'
if ((Get-AppxPackage -Name $name) -or (Test-Path -LiteralPath 'HKCU:\Software\LiteView')) {
    throw 'Refusing to disturb an existing LiteView installation or registration.'
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("2e941141-7f97-4756-ba1d-9decde894a3d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IApplicationActivationManager {
    [PreserveSig] int ActivateApplication([MarshalAs(UnmanagedType.LPWStr)] string id,
        [MarshalAs(UnmanagedType.LPWStr)] string args, uint options, out uint processId);
}
[ComImport, Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")]
class ApplicationActivationManager {}
public static class LiteViewActivation {
    public static uint Run(string id, string args) {
        var manager = (IApplicationActivationManager)new ApplicationActivationManager();
        try {
            uint pid;
            int hr = manager.ActivateApplication(id, args, 0, out pid);
            Marshal.ThrowExceptionForHR(hr);
            return pid;
        } finally { Marshal.ReleaseComObject(manager); }
    }
}
'@
function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Invoke-Classic([string]$Arguments) {
    $process = Start-Process -FilePath $Binary -ArgumentList $Arguments -PassThru -Wait -WindowStyle Hidden
    Assert-True ($process.ExitCode -eq 0) "Classic command failed: $Arguments"
}
function Invoke-Packaged([string]$Arguments) {
    $processId = [LiteViewActivation]::Run($aumid, $Arguments)
    try { $process = [Diagnostics.Process]::GetProcessById($processId) } catch { return }
    if (-not $process.WaitForExit(30000)) { throw "Package activation did not exit: $Arguments" }
    Assert-True ($process.ExitCode -eq 0) "Packaged command failed: $Arguments"
}
function Get-Diagnostics([string]$Label) {
    $report = Join-Path $root "$Label.json"
    Invoke-Packaged ('--diagnostics "' + $report + '"')
    Assert-True (Test-Path -LiteralPath $report) 'Packaged activation did not produce a diagnostics report.'
    $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    Assert-True $result.packaged 'Activation ran without package identity.'
    Assert-True ($result.package_family -eq $family) 'Wrong package family.'
    Assert-True ($result.application_id -eq $aumid) 'Wrong application identity.'
    Assert-True ($result.package_local_folder -and $result.config_dir -eq $result.package_local_folder) 'Settings did not use the actual WinRT LocalFolder.'
    return $result
}
function Sign-TestCopy([string]$Source, [string]$Label) {
    $copy = Join-Path $root "$Label.msix"
    Copy-Item -LiteralPath $Source -Destination $copy
    & $SignTool sign /fd SHA256 /sha1 $cert.Thumbprint $copy | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Signing disposable test copy failed.' }
    & $SignTool verify /pa /v $copy | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Disposable package signature verification failed.' }
    return $copy
}
$savedConfig = $env:LITEVIEW_CONFIG_DIR
$cert = $null
$classicRegistered = $false
try {
    $env:LITEVIEW_CONFIG_DIR = Join-Path $root 'classic-config'
    $classicRegistered = $true
    Invoke-Classic '--register'
    $classicCommand = (Get-Item -LiteralPath $classicKey).GetValue('')
    Remove-Item Env:\LITEVIEW_CONFIG_DIR
    $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -KeyExportPolicy NonExportable `
        -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddDays(2) `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3') -FriendlyName 'LiteView disposable CI test only'
    $cer = Join-Path $root 'test-only.cer'
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    # Trust only in this disposable runner's TrustedPeople store; never install a root CA.
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
    $signed = Sign-TestCopy $Package 'install'
    Add-AppxPackage -Path $signed
    $first = Get-Diagnostics 'installed'
    $config = Join-Path $first.config_dir 'settings.json'
    New-Item -ItemType Directory -Path $first.config_dir -Force | Out-Null
    Set-Content -LiteralPath $config -Value '{"language":"En","ask_default_app":false}' -Encoding ascii
    $before = Get-Content -LiteralPath $config -Raw
    Invoke-Packaged '--register'
    Invoke-Packaged '--unregister'
    Assert-True ((Get-Item -LiteralPath $classicKey).GetValue('') -eq $classicCommand) 'Store commands changed classic registration.'
    $original = Get-Content -LiteralPath ([IO.Path]::ChangeExtension($Package, '.json')) -Raw | ConvertFrom-Json
    $parts = $original.identity.Version.Split('.')
    $upgradeVersion = '{0}.{1}.{2}.0' -f $parts[0], $parts[1], ([int]$parts[2] + 1)
    $upgradeOutput = Join-Path $root 'upgrade'
    & python (Join-Path $PSScriptRoot 'msix.py') --binary $Binary --store-version $upgradeVersion --makeappx $MakeAppx --output $upgradeOutput
    if ($LASTEXITCODE -ne 0) { throw 'Upgrade test package build failed.' }
    $upgrade = Sign-TestCopy (Join-Path $upgradeOutput "LiteView-store-$upgradeVersion-x64.msix") 'upgrade'
    Add-AppxPackage -Path $upgrade
    $second = Get-Diagnostics 'upgraded'
    Assert-True ($first.config_dir -eq $second.config_dir) 'Upgrade moved configuration.'
    Assert-True ((Get-Content -LiteralPath $config -Raw) -eq $before) 'Upgrade changed preferences.'
    $image = Join-Path $root 'user-image.png'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\assets\app.png') -Destination $image
    Get-AppxPackage -Name $name | Where-Object PackageFamilyName -eq $family | Remove-AppxPackage
    Assert-True (-not (Get-AppxPackage -Name $name)) 'Package remained installed.'
    Assert-True (Test-Path -LiteralPath $image) 'Uninstall deleted a user image.'
    Assert-True ((Get-Item -LiteralPath $classicKey).GetValue('') -eq $classicCommand) 'Uninstall changed classic registration.'
    Write-Host 'PASS: signed test install, real AUMID activation, isolated settings, protected classic registration, upgrade and uninstall.'
} finally {
    Get-AppxPackage -Name $name | Where-Object PackageFamilyName -eq $family | Remove-AppxPackage -ErrorAction Continue
    if ($classicRegistered) {
        $env:LITEVIEW_CONFIG_DIR = Join-Path $root 'classic-config'
        Invoke-Classic '--unregister'
    }
    $env:LITEVIEW_CONFIG_DIR = $savedConfig
    if ($cert) {
        Remove-Item -LiteralPath "Cert:\LocalMachine\TrustedPeople\$($cert.Thumbprint)" -ErrorAction Continue
        Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($cert.Thumbprint)" -ErrorAction Continue
    }
}
