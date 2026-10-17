param(
    [Parameter(Mandatory = $true)][string]$Package,
    [Parameter(Mandatory = $true)][string]$Output,
    [switch]$GuiReplay
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'WACK installs tools and changes certificate trust. Run only on a disposable GitHub-hosted Windows runner.'
}
$Package = (Resolve-Path -LiteralPath $Package).Path
$Output = [IO.Path]::GetFullPath($Output)
$tempPrefix = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (-not $Output.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $Output)) {
    throw 'Reports must use a new directory inside RUNNER_TEMP.'
}
New-Item -ItemType Directory -Path $Output | Out-Null
# Signed copies and certificates are deliberately outside the uploaded report directory.
$root = Join-Path $env:RUNNER_TEMP ('liteview-wack-private-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$expectedPackageHash = '2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06'
$expectedBinaryHash = '21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a'
$name = 'ice.LiteView'
$family = 'ice.LiteView_rz8m2sa64m4em'
$publisher = 'CN=56079E3F-5861-4D9C-B9C3-93F76224C42E'
$cert = $null
$packageMayBeInstalled = $false
$state = [ordered]@{
    status = 'inconclusive'; stage = 'preflight'; error = $null; cleanup_errors = @()
    scope = 'Supplemental GitHub Windows runner only; not Windows 10/11 acceptance or Store certification'
    original_msix_sha256 = $expectedPackageHash; binary_sha256 = $expectedBinaryHash
    source_release_run = '39027759847'; signed_copy_sha256 = $null; gui_status = 'not-run'
}
function Save-Json($Value, [string]$Name) {
    $Value | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $Output $Name) -Encoding utf8
}
function Assert-Hash([string]$Path, [string]$Expected) {
    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $Expected) { throw "SHA256 mismatch: $Path ($actual)" }
}
function Assert-MicrosoftSignature([string]$Path) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:^|,\s*)O=Microsoft Corporation(?:,|$)') {
        throw "Expected valid Microsoft Authenticode signature: $Path"
    }
    return [ordered]@{ status = [string]$signature.Status; subject = $signature.SignerCertificate.Subject
        thumbprint = $signature.SignerCertificate.Thumbprint; sha256 = (Get-FileHash -LiteralPath $Path).Hash }
}
function Invoke-Recorded([string]$File, [string]$Arguments, [string]$Label, [int]$TimeoutSeconds) {
    $started = Get-Date
    $record = [ordered]@{ file = $File; arguments = $Arguments; started_utc = $started.ToUniversalTime().ToString('o')
        timeout_seconds = $TimeoutSeconds; timed_out = $false; exit_code = $null }
    $process = $null
    try {
        $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput (Join-Path $Output "$Label.stdout.log") `
            -RedirectStandardError (Join-Path $Output "$Label.stderr.log")
        $null = $process.Handle
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $record.timed_out = $true
            # Bound only the process tree started by this invocation.
            & taskkill.exe /PID $process.Id /T /F | Out-Host
            throw "Timed out: $Label"
        }
        $record.exit_code = $process.ExitCode
        if ($null -eq $record.exit_code) { throw "Cannot read process exit code: $Label" }
        return $record.exit_code
    } finally {
        $record['finished_utc'] = (Get-Date).ToUniversalTime().ToString('o')
        Save-Json $record "$Label.command.json"
        if ($process) { $process.Dispose() }
    }
}
try {
    Assert-Hash $Package $expectedPackageHash
    if (Get-AppxPackage -Name $name) { throw 'Refusing to disturb an existing LiteView package.' }
    if ($env:LITEVIEW_SCRIPT -or $env:LITEVIEW_CONFIG_DIR) { throw 'Unexpected app replay/config environment would contaminate WACK.' }
    $os = Get-CimInstance Win32_OperatingSystem
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $admin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    Save-Json ([ordered]@{ os = $os.Caption; version = $os.Version; build = $os.BuildNumber
        session_id = [Diagnostics.Process]::GetCurrentProcess().SessionId; user_interactive = [Environment]::UserInteractive
        administrator = $admin; runner_os = $env:RUNNER_OS; image_os = $env:ImageOS; image_version = $env:ImageVersion
        powershell = $PSVersionTable.PSVersion.ToString(); commit = $env:GITHUB_SHA; run_id = $env:GITHUB_RUN_ID }) 'environment.json'
    if (-not $admin) { throw 'WACK CLI requires an administrator.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $payload = Join-Path $root 'payload'
    [IO.Compression.ZipFile]::ExtractToDirectory($Package, $payload)
    Assert-Hash (Join-Path $payload 'LiteView.exe') $expectedBinaryHash
    $manifest = Join-Path $payload 'AppxManifest.xml'
    [xml]$xml = Get-Content -LiteralPath $manifest -Raw
    if ($xml.Package.Identity.Name -ne $name -or $xml.Package.Identity.Publisher -ne $publisher -or
        $xml.Package.Identity.Version -ne '1.0.0.0' -or $xml.Package.Identity.ProcessorArchitecture -ne 'x64') {
        throw 'Unexpected release package identity.'
    }
    Copy-Item -LiteralPath $manifest -Destination (Join-Path $Output 'AppxManifest.xml')

    $state.stage = 'tool-installation'
    $appcert = "${env:ProgramFiles(x86)}\Windows Kits\10\App Certification Kit\appcert.exe"
    if (-not (Test-Path -LiteralPath $appcert)) {
        $installer = Join-Path $root 'winsdksetup.exe'
        $uri = 'https://download.microsoft.com/download/46742ab5-6592-4968-a793-129e7f3bc55a/KIT_BUNDLE_WINDOWSSDK_MEDIACREATION/winsdksetup.exe'
        Invoke-WebRequest -Uri $uri -OutFile $installer -TimeoutSec 180
        Assert-Hash $installer 'b0bdbad38ae74c40ccae8cdbcf229807627fba2bc3100be28301c4ba884f755e'
        Save-Json (Assert-MicrosoftSignature $installer) 'sdk-installer-signature.json'
        $code = Invoke-Recorded $installer ('/features OptionId.WindowsSoftwareLogoToolkit /quiet /norestart /log "' + (Join-Path $Output 'sdk-install.log') + '"') 'sdk-install' 900
        if ($code -ne 0) { throw "SDK installation did not complete without a reboot: $code" }
    }
    if (-not (Test-Path -LiteralPath $appcert)) { throw 'WACK installation did not provide appcert.exe.' }
    Save-Json ([ordered]@{ path = $appcert; file_version = (Get-Item -LiteralPath $appcert).VersionInfo.FileVersion
        signature = (Assert-MicrosoftSignature $appcert) }) 'appcert-version.json'
    $null = Invoke-Recorded $appcert '/?' 'appcert-help' 60
    $signTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signTool) { throw 'Windows SDK signtool is missing.' }

    $state.stage = 'test-signature'
    $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -KeyExportPolicy NonExportable `
        -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddDays(2) `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3') -FriendlyName 'LiteView disposable WACK only'
    $cer = Join-Path $root 'test-only.cer'
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
    $signed = Join-Path $root 'LiteView-test-only.msix'
    Copy-Item -LiteralPath $Package -Destination $signed
    $code = Invoke-Recorded $signTool.FullName ('sign /fd SHA256 /sha1 ' + $cert.Thumbprint + ' "' + $signed + '"') 'sign-test-copy' 120
    if ($code -ne 0) { throw "Test signing failed: $code" }
    $code = Invoke-Recorded $signTool.FullName ('verify /pa /v "' + $signed + '"') 'verify-test-copy' 120
    if ($code -ne 0) { throw "Test signature verification failed: $code" }
    $signedPayload = Join-Path $root 'signed-payload'
    [IO.Compression.ZipFile]::ExtractToDirectory($signed, $signedPayload)
    Assert-Hash (Join-Path $signedPayload 'LiteView.exe') $expectedBinaryHash
    Assert-Hash (Join-Path $signedPayload 'AppxManifest.xml') (Get-FileHash -LiteralPath $manifest).Hash.ToLowerInvariant()
    $state.signed_copy_sha256 = (Get-FileHash -LiteralPath $signed).Hash.ToLowerInvariant()
    Save-Json $state 'validation.json'

    $state.stage = 'wack'
    $code = Invoke-Recorded $appcert 'reset' 'appcert-reset' 120
    if ($code -ne 0) { throw "WACK reset failed: $code" }
    $packageMayBeInstalled = $true
    $report = Join-Path $Output 'wack.xml'
    $code = Invoke-Recorded $appcert ('test -appxpackagepath "' + $signed + '" -reportoutputpath "' + $report + '"') 'appcert-test' 1800
    $state.stage = 'report-review'
    & python (Join-Path $PSScriptRoot 'wack_report.py') --report $report "--exit-code=$code" --output (Join-Path $Output 'wack-summary.json')
    $summaryCode = $LASTEXITCODE
    $summary = Get-Content -LiteralPath (Join-Path $Output 'wack-summary.json') -Raw | ConvertFrom-Json
    $state.status = $summary.status
    if ($summaryCode -ne 0) { throw "WACK $($summary.status): $($summary.reason)" }
    if ($GuiReplay) {
        $state.stage = 'installed-gui-replay'
        Add-AppxPackage -Path $signed
        & (Join-Path $PSScriptRoot 'test_store_gui.ps1') -Output (Join-Path $Output 'gui')
        $state.gui_status = 'pass'
    }
    $state.stage = 'completed'
} catch {
    if ($state.stage -eq 'installed-gui-replay') { $state.gui_status = 'fail' }
    $state.error = $_.Exception.Message
    throw
} finally {
    if ($packageMayBeInstalled) {
        try { Get-AppxPackage -Name $name | Where-Object PackageFamilyName -eq $family | Remove-AppxPackage }
        catch { $state.cleanup_errors += $_.Exception.Message }
    }
    if ($cert) {
        foreach ($store in @('Cert:\LocalMachine\TrustedPeople', 'Cert:\CurrentUser\My')) {
            try { Remove-Item -LiteralPath "$store\$($cert.Thumbprint)" -ErrorAction Stop }
            catch { $state.cleanup_errors += $_.Exception.Message }
        }
    }
    try { Assert-Hash $Package $expectedPackageHash }
    catch { $state.cleanup_errors += $_.Exception.Message }
    if ($state.cleanup_errors.Count) { $state.status = 'inconclusive' }
    Save-Json $state 'validation.json'
    if ($env:GITHUB_STEP_SUMMARY) {
        @('# Supplemental Store package validation', '', "Status: $($state.status)", "Stage: $($state.stage)",
            '', $state.scope, '', 'Raw reports, commands, hashes and environment are in the wack-reports artifact.',
            'Test-signed packages, certificates and private keys are not uploaded.') | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    }
    if ($state.cleanup_errors.Count) { throw ('Cleanup/input-integrity check failed: ' + ($state.cleanup_errors -join '; ')) }
}
