# Read-only support collection. Never launches LiteView or uploads data.
[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$Binary,
    [ValidateRange(1, 30)][int]$Days = 7
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertFrom-LiteViewCrashEvent {
    param([xml]$Xml, [datetime]$TimeCreated)

    $data = @{}
    foreach ($item in $Xml.SelectNodes("/*[local-name()='Event']/*[local-name()='EventData']/*[local-name()='Data']")) {
        $data[$item.GetAttribute('Name')] = $item.InnerText
    }
    if ($data['AppName'] -notmatch '^(?i:LiteView\.exe)$') { return $null }

    # Deliberately exclude AppPath, ModulePath, report IDs, SIDs and raw messages.
    # Filename-only module names preserve the useful crash signature without paths.
    $module = ($data['ModuleName'] -split '[/\\]')[-1]
    [ordered]@{
        occurredAtUtc = $TimeCreated.ToUniversalTime().ToString('o')
        application = 'LiteView.exe'
        applicationVersion = $data['AppVersion']
        module = $module
        moduleVersion = $data['ModuleVersion']
        exceptionCode = $data['ExceptionCode']
        faultingOffset = $data['FaultingOffset']
    }
}

function Get-LiteViewBinaryInfo {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path
    if ($item.PSIsContainer -or $item.Name -ine 'LiteView.exe') {
        throw 'Binary must point to LiteView.exe.'
    }
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($item.FullName)
    [ordered]@{
        filename = $item.Name
        productVersion = $version.ProductVersion
        fileVersion = $version.FileVersion
        bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Write-LiteViewSupportReport {
    param([string]$Destination, [string]$Exe, [int]$LookbackDays)

    if (-not $Destination) {
        $name = 'LiteView-support-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 6))
        $Destination = Join-Path $PSScriptRoot $name
    }
    $Destination = [IO.Path]::GetFullPath($Destination)
    $reportPath = Join-Path $Destination 'support-report.json'
    if (Test-Path -LiteralPath $reportPath) { throw 'Report already exists; choose a new output directory.' }
    $null = New-Item -ItemType Directory -Path $Destination -Force

    $warnings = [Collections.Generic.List[string]]::new()
    $system = $null
    try {
        $win = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
        $os = Get-CimInstance Win32_OperatingSystem
        $system = [ordered]@{
            caption = $os.Caption
            architecture = $os.OSArchitecture
            displayVersion = $win.DisplayVersion
            build = '{0}.{1}' -f $win.CurrentBuildNumber, $win.UBR
            edition = $win.EditionID
        }
    } catch { $warnings.Add('Windows version query unavailable.') }

    $gpu = @()
    try {
        $gpu = @(Get-CimInstance Win32_VideoController | ForEach-Object {
            [ordered]@{ name = $_.Name; driverVersion = $_.DriverVersion; driverDate = [string]$_.DriverDate }
        })
    } catch { $warnings.Add('Display adapter query unavailable.') }

    $packages = @()
    try {
        $packages = @(Get-AppxPackage -Name 'ice.LiteView' | ForEach-Object {
            [ordered]@{ name = $_.Name; version = [string]$_.Version; architecture = [string]$_.Architecture; status = [string]$_.Status }
        })
    } catch { $warnings.Add('Store package query unavailable; this does not prove that no package is installed.') }

    $binaryInfo = $null
    if ($Exe) {
        # Inspect bytes and version resources only, never run a supplied executable.
        $binaryInfo = Get-LiteViewBinaryInfo -Path $Exe
    }

    $eventStatus = 'available'
    $events = @()
    $records = @()
    try {
        $records = @(Get-WinEvent -FilterHashtable @{
            LogName = 'Application'; Id = 1000; StartTime = (Get-Date).AddDays(-$LookbackDays)
        } -MaxEvents 500 -ErrorAction Stop)
        $events = @($records | ForEach-Object {
            ConvertFrom-LiteViewCrashEvent -Xml ([xml]$_.ToXml()) -TimeCreated $_.TimeCreated
        } | Where-Object { $null -ne $_ })
    } catch {
        if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') {
            $eventStatus = 'no-application-error-events'
        } else {
            $eventStatus = 'query-failed'
            $warnings.Add('Application Error event query unavailable; no crash verdict can be drawn.')
        }
    }
    if ($records.Count -eq 500) {
        $warnings.Add('Only the 500 newest Application Error events were inspected; older LiteView events may be omitted.')
    }

    $report = [ordered]@{
        schemaVersion = 1
        collectedAtUtc = [datetime]::UtcNow.ToString('o')
        collection = 'read-only; no application launch, settings changes, image access or upload'
        diagnosis = 'not-determined'
        windows = $system
        displayAdapters = $gpu
        currentUserStorePackages = $packages
        suppliedBinary = $binaryInfo
        eventQuery = [ordered]@{ status = $eventStatus; lookbackDays = $LookbackDays; maximumEvents = 500; matchingCrashes = $events.Count }
        crashes = $events
        warnings = @($warnings.ToArray())
    }
    $json = $report | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText($reportPath, $json + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    Write-Host ('Report: {0}' -f $reportPath)
    Write-Host ('Matching LiteView crashes: {0}. No data was uploaded.' -f $events.Count)
    if (-not $events.Count) {
        Write-Host 'No matching crash record is NOT a passing startup test. Please include the crash details screenshot.'
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    Write-LiteViewSupportReport -Destination $OutputDirectory -Exe $Binary -LookbackDays $Days
}
