# Disposable Windows GitHub runner only. Never run against a developer's registrations.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$PreviousInstaller,
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or -not $env:RUNNER_TEMP) {
    throw 'Installer smoke tests require a disposable Windows GitHub Actions runner.'
}
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$PreviousInstaller = (Resolve-Path -LiteralPath $PreviousInstaller).Path
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8B908BC1-B618-42A7-9DF4-5E269C59C90D}_is1'
$appKey = 'HKCU:\Software\Classes\Applications\LiteView.exe'
$capabilitiesKey = 'HKCU:\Software\LiteView'
$commandKey = "$appKey\shell\open\command"
foreach ($key in @($uninstallKey, $appKey, $capabilitiesKey)) {
    if (Test-Path -LiteralPath $key) { throw "Refusing to test over existing registration: $key" }
}
if (Get-ChildItem -LiteralPath 'HKCU:\Software\Classes' | Where-Object PSChildName -Like 'LiteView.*') {
    throw 'Refusing to test over existing LiteView ProgIDs.'
}
$runnerTemp = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')
$root = Join-Path $runnerTemp 'liteview-installer-smoke'
New-Item -ItemType Directory -Path $root | Out-Null # No reuse of prior test state.
$app = Join-Path $root 'installed'
$config = Join-Path $root 'config'
$portable = Join-Path $root 'portable'
New-Item -ItemType Directory -Path $config, $portable | Out-Null
Copy-Item -LiteralPath $Binary -Destination (Join-Path $portable 'LiteView.exe')
$portableExe = Join-Path $portable 'LiteView.exe'
$oldConfig = $env:LITEVIEW_CONFIG_DIR
$oldScript = $env:LITEVIEW_SCRIPT
$oldLog = $env:LITEVIEW_LOG
$env:LITEVIEW_CONFIG_DIR = $config
$env:LITEVIEW_SCRIPT = $null
$env:LITEVIEW_LOG = Join-Path $root 'app.log'
Set-Content -LiteralPath (Join-Path $config 'settings.json') -Encoding utf8 -Value '{"single_instance":false,"ask_default_app":false}'
Set-Content -LiteralPath (Join-Path $config 'user-sentinel.txt') -Value 'preserve my settings'
$expectedHash = (Get-FileHash -LiteralPath $Binary).Hash
$seq = 0

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Get-OpenCommand {
    if (Test-Path -LiteralPath $commandKey) { return (Get-Item -LiteralPath $commandKey).GetValue('') }
    return $null
}
function Invoke-Checked([string]$Exe, [string[]]$Arguments) {
    # Inno delegates cleanup to a child. Wait for the whole process tree, not
    # just the launcher; the dedicated CI lifecycle step has a five-minute timeout.
    $p = Start-Process -FilePath $Exe -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru
    Assert-True ($p.ExitCode -eq 0) "Process exited $($p.ExitCode): $Exe"
}
function Install-Test([string]$Setup, [bool]$Associate) {
    $script:seq++
    $tasks = if ($Associate) { 'associate' } else { '' }
    Invoke-Checked $Setup @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS',
        ('/DIR="{0}"' -f $app), ('/TASKS="{0}"' -f $tasks),
        ('/LOG="{0}"' -f (Join-Path $root "install-$seq.log")))
    foreach ($name in @('LiteView.exe', 'LICENSE', 'README.md', 'THIRD-PARTY-NOTICES.md', 'LICENSES.html', 'INSTALL.md', 'unins000.exe')) {
        Assert-True (Test-Path -LiteralPath (Join-Path $app $name)) "Missing installed file: $name"
    }
    Assert-True ((Get-FileHash -LiteralPath (Join-Path $app 'LiteView.exe')).Hash -eq $expectedHash) 'Installed executable differs from tested build.'
}
function Uninstall-Test {
    # The uninstaller recursively removes installed files. Check the exact target first.
    $resolved = [IO.Path]::GetFullPath($app)
    Assert-True ($resolved.StartsWith($runnerTemp + '\', [StringComparison]::OrdinalIgnoreCase)) 'Unsafe uninstall directory.'
    Assert-True ($resolved -eq (Join-Path $root 'installed')) 'Unexpected uninstall directory.'
    if (Test-Path -LiteralPath (Join-Path $app 'unins000.exe')) {
        $script:seq++
        Invoke-Checked (Join-Path $app 'unins000.exe') @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
            ('/LOG="{0}"' -f (Join-Path $root "uninstall-$seq.log")))
        # The registry key disappears before child cleanup ends: it is not a wait signal.
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $app 'unins000.exe'))) 'Uninstaller cleanup is incomplete.'
        Assert-True (-not (Test-Path -LiteralPath $uninstallKey)) 'Uninstall registration remains.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $app 'LiteView.exe'))) 'Installed executable remains.'
    }
}
function Assert-Preserved {
    Assert-True ((Get-Content -LiteralPath (Join-Path $config 'user-sentinel.txt') -Raw).Trim() -eq 'preserve my settings') 'User configuration was removed.'
    Assert-True ((Get-FileHash -LiteralPath (Join-Path $app 'user-image.png')).Hash -eq $photoHash) 'User image was removed or modified.'
}

try {
    # A synthetic prior installer version exercises the same AppId's upgrade path.
    Install-Test $PreviousInstaller $true
    Assert-True ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -eq '0.0.0') 'Prior fixture version not installed.'
    Assert-True ((Get-OpenCommand) -eq ('"' + (Join-Path $app 'LiteView.exe') + '" "%1"')) 'Installed association not registered.'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../tests/fixtures/animation.png') -Destination (Join-Path $app 'user-image.png')
    $photoHash = (Get-FileHash -LiteralPath (Join-Path $app 'user-image.png')).Hash
    Install-Test $Installer $true
    Assert-True ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -eq $Version) 'Upgrade did not update installer version.'
    Assert-Preserved
    Uninstall-Test
    Assert-True (-not (Test-Path -LiteralPath $appKey)) 'Uninstall did not remove owned association.'
    Assert-True (-not (Test-Path -LiteralPath $capabilitiesKey)) 'Uninstall did not remove owned capabilities.'
    Assert-Preserved

    # A portable copy owns the association; opting out of association must preserve it.
    Invoke-Checked $portableExe @('--register')
    $portableCommand = Get-OpenCommand
    Assert-True ($portableCommand -eq ('"' + $portableExe + '" "%1"')) 'Portable registration failed.'
    Install-Test $Installer $false
    Assert-True ((Get-OpenCommand) -eq $portableCommand) 'No-association install changed portable registration.'
    Uninstall-Test
    Assert-True ((Get-OpenCommand) -eq $portableCommand) 'Uninstall removed another copy association.'
    Assert-Preserved

    # A portable copy takes over after installation: uninstall must still leave it alone.
    Install-Test $Installer $true
    Invoke-Checked $portableExe @('--register')
    Uninstall-Test
    Assert-True ((Get-OpenCommand) -eq $portableCommand) 'Uninstall removed a later portable registration.'
    Assert-Preserved
    Write-Host 'PASS: install, synthetic version upgrade, uninstall, user-data preservation, portable association ownership.'
}
finally {
    try {
        Uninstall-Test
        Invoke-Checked $portableExe @('--unregister')
    }
    finally {
        $env:LITEVIEW_CONFIG_DIR = $oldConfig
        $env:LITEVIEW_SCRIPT = $oldScript
        $env:LITEVIEW_LOG = $oldLog
    }
}
