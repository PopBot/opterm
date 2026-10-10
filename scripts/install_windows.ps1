# Run from an extracted native Windows package; installs for the current user.
param([switch]$DesktopShortcut)
$ErrorActionPreference = 'Stop'
$Destination = Join-Path $env:LOCALAPPDATA 'Opterm'
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
Get-ChildItem -LiteralPath $PSScriptRoot | Where-Object { $_.Name -ne 'install.ps1' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $Destination -Recurse -Force
}
$Shell = New-Object -ComObject WScript.Shell
$Programs = [Environment]::GetFolderPath('Programs')
function Add-OptermShortcut($Path) {
    $Shortcut = $Shell.CreateShortcut($Path)
    $Shortcut.TargetPath = Join-Path $Destination 'opterm.exe'
    $Shortcut.IconLocation = "$Destination\opterm.exe,0"
    $Shortcut.Description = 'Opterm Terminal'
    $Shortcut.Save()
}
Add-OptermShortcut (Join-Path $Programs 'Opterm.lnk')
if ($DesktopShortcut) {
    Add-OptermShortcut (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Opterm.lnk')
}
Write-Host "Installed Opterm in $Destination with a Start menu shortcut."
