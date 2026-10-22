# Pure event fixtures: no live event-log reads or application launches.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'collect_windows_support.ps1')

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$fixture = [xml]@'
<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
  <System><EventID>1000</EventID></System>
  <EventData>
    <Data Name="AppName">LiteView.exe</Data>
    <Data Name="AppVersion">1.0.0.0</Data>
    <Data Name="ModuleName">C:\Users\private-user\vendor\driver.dll</Data>
    <Data Name="ModuleVersion">31.0.0.1</Data>
    <Data Name="ExceptionCode">c0000005</Data>
    <Data Name="FaultingOffset">0000000000012345</Data>
    <Data Name="AppPath">C:\Users\private-user\Downloads\LiteView.exe</Data>
    <Data Name="ModulePath">C:\Users\private-user\vendor\driver.dll</Data>
    <Data Name="ReportId">private-report-id</Data>
    <Data Name="ImagePath">D:\private-photo.png</Data>
  </EventData>
</Event>
'@
$result = ConvertFrom-LiteViewCrashEvent -Xml $fixture -TimeCreated ([datetime]'2026-10-20T02:00:00Z')
Assert-True ($result.application -eq 'LiteView.exe') 'Lost application identity.'
Assert-True ($result.module -eq 'driver.dll') 'Module filename must not contain a directory.'
Assert-True ($result.exceptionCode -eq 'c0000005') 'Lost the actual exception code.'
Assert-True ($result.faultingOffset -eq '0000000000012345') 'Lost the faulting offset.'
$json = $result | ConvertTo-Json
Assert-True ($json -notmatch 'private-|ImagePath|AppPath|ReportId|ModulePath') 'Private or non-allowlisted event data leaked.'
Write-Host 'PASS: useful crash signature retained; private paths and unrelated fields excluded.'

$other = [xml]($fixture.OuterXml.Replace('LiteView.exe', 'AnotherApp.exe'))
Assert-True ($null -eq (ConvertFrom-LiteViewCrashEvent -Xml $other -TimeCreated (Get-Date))) 'Unrelated application event leaked.'
$similar = [xml]($fixture.OuterXml.Replace('LiteView.exe', 'NotLiteView.exe'))
Assert-True ($null -eq (ConvertFrom-LiteViewCrashEvent -Xml $similar -TimeCreated (Get-Date))) 'Substring-only application match leaked.'
$lower = [xml]($fixture.OuterXml.Replace('LiteView.exe', 'liteview.exe'))
Assert-True ($null -ne (ConvertFrom-LiteViewCrashEvent -Xml $lower -TimeCreated (Get-Date))) 'Case-insensitive filename match failed.'
Write-Host 'PASS: only the exact LiteView executable is included, case-insensitively.'

$minimal = [xml]'<Event><EventData><Data Name="AppName">LiteView.exe</Data></EventData></Event>'
$partial = ConvertFrom-LiteViewCrashEvent -Xml $minimal -TimeCreated (Get-Date)
Assert-True ($null -eq $partial.exceptionCode) 'Missing exception code must not be invented.'
Write-Host 'PASS: incomplete records remain explicitly incomplete.'
