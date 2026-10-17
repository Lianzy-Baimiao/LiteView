# Read-only check of the linked executable; never starts the app or changes associations.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Binary,
    [string]$Version
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Binary = (Resolve-Path -LiteralPath $Binary).Path

if (-not ('LiteViewResourceProbe' -as [type])) {
    Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class LiteViewResourceProbe {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern uint ExtractIconEx(string file, int index, IntPtr[] large, IntPtr[] small, uint count);
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryEx(string file, IntPtr handle, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")]
    private static extern bool FreeLibrary(IntPtr module);

    public static byte[] ReadManifest(string file) {
        // DATAFILE | IMAGE_RESOURCE: map resources without executing any code.
        IntPtr module = LoadLibraryEx(file, IntPtr.Zero, 0x22);
        if (module == IntPtr.Zero) throw new Win32Exception();
        try {
            IntPtr resource = FindResource(module, new IntPtr(1), new IntPtr(24));
            if (resource == IntPtr.Zero) throw new InvalidOperationException("Missing application manifest #1.");
            uint length = SizeofResource(module, resource);
            if (length == 0 || length > 1048576) throw new InvalidOperationException("Invalid manifest length.");
            IntPtr loaded = LoadResource(module, resource);
            if (loaded == IntPtr.Zero) throw new Win32Exception();
            IntPtr data = LockResource(loaded);
            if (data == IntPtr.Zero) throw new InvalidOperationException("Cannot read manifest.");
            byte[] bytes = new byte[(int)length];
            Marshal.Copy(data, bytes, 0, bytes.Length);
            return bytes;
        } finally {
            FreeLibrary(module);
        }
    }
}
'@
}

$icons = [LiteViewResourceProbe]::ExtractIconEx($Binary, -1, $null, $null, 0)
if ($icons -eq 0 -or $icons -eq [uint32]::MaxValue) {
    throw "Executable has no extractable icon: $Binary"
}
$large = New-Object IntPtr[] 1
$small = New-Object IntPtr[] 1
try {
    $extracted = [LiteViewResourceProbe]::ExtractIconEx($Binary, 0, $large, $small, 1)
    if ($extracted -eq 0 -or $large[0] -eq [IntPtr]::Zero -or $small[0] -eq [IntPtr]::Zero) {
        throw 'Cannot decode the executable icon at large and small shell sizes.'
    }
} finally {
    foreach ($icon in @($large[0], $small[0])) {
        if ($icon -ne [IntPtr]::Zero) { [void][LiteViewResourceProbe]::DestroyIcon($icon) }
    }
}
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
if ($info.FileDescription -ne 'LiteView' -or $info.ProductName -ne 'LiteView' -or $info.OriginalFilename -ne 'LiteView.exe') {
    throw "Executable is missing LiteView version metadata: $Binary"
}
if (-not $info.FileVersion -or -not $info.ProductVersion) {
    throw 'Executable is missing its version number.'
}
if ($Version -and ($info.FileVersion -ne $Version -or $info.ProductVersion -ne $Version)) {
    throw "Executable version differs from expected $Version."
}
$manifest = New-Object System.Xml.XmlDocument
$manifest.XmlResolver = $null
$manifest.LoadXml([Text.Encoding]::UTF8.GetString([LiteViewResourceProbe]::ReadManifest($Binary)))
$namespaces = New-Object System.Xml.XmlNamespaceManager($manifest.NameTable)
$namespaces.AddNamespace('asm', 'urn:schemas-microsoft-com:asm.v1')
$namespaces.AddNamespace('dpi', 'http://schemas.microsoft.com/SMI/2016/WindowsSettings')
$checks = @{
    '//dpi:dpiAwareness' = 'PerMonitorV2'
    '//dpi:longPathAware' = 'true'
    '/asm:assembly/asm:assemblyIdentity/@name' = 'LiteView'
    '//asm:dependentAssembly/asm:assemblyIdentity/@name' = 'Microsoft.Windows.Common-Controls'
}
foreach ($xpath in $checks.Keys) {
    $node = $manifest.SelectSingleNode($xpath, $namespaces)
    if ($null -eq $node -or $node.get_InnerText() -ne $checks[$xpath]) {
        throw "Executable manifest is missing required setting: $xpath"
    }
}
Write-Host "PASS: $icons icon group(s), LiteView $($info.ProductVersion), PerMonitorV2, long paths, common controls."
