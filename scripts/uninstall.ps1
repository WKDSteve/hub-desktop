<#
.SYNOPSIS
Remove hub-desktop for the current user on Windows, without administrator rights.

.DESCRIPTION
Asks the installed program to take its place in the system away (`hub-desktop uninstall`: stop the
hub, the login entry, the link on the desktop), then removes the program file from the install
folder (%LOCALAPPDATA%\hub-desktop\bin, or $env:HUB_DESKTOP_INSTALL_DIR). Your settings stay
unless you ask: -PurgeSettings removes the files of the app in the settings folder. Other apps are
never touched. A second run finds nothing to do and says so; it is not an error.

If taking the system entries away fails, the program is NOT removed (a login entry that points at
a missing program is worse than a program that is still there): read the message and run it again.

.PARAMETER PurgeSettings
Also remove the settings of the app (%APPDATA%\hub-desktop, only the files the app makes).

.PARAMETER AllowElevated
Remove although the session is elevated (for a build machine).

.EXAMPLE
.\uninstall.ps1 -PurgeSettings
#>
[CmdletBinding()]
param(
    [switch] $PurgeSettings,
    [switch] $AllowElevated
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Exit-Uninstall([string] $Message) {
    throw "uninstall.ps1: $Message"
}

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $AllowElevated -and $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Exit-Uninstall 'do not run this elevated: it removes the installation of one user (-AllowElevated for a build machine)'
}

if ($env:HUB_DESKTOP_INSTALL_DIR) {
    $dir = $env:HUB_DESKTOP_INSTALL_DIR
}
else {
    $dir = Join-Path $env:LOCALAPPDATA 'hub-desktop\bin'
}
$program = Join-Path $dir 'hub-desktop.exe'
if (-not (Test-Path -LiteralPath $program -PathType Leaf)) {
    Write-Output "nothing to remove: no hub-desktop.exe in $dir"
    return
}

$flags = @()
if ($PurgeSettings) { $flags += '--purge-settings' }
# The program takes the system entries away itself (it knows where they are). A program from before
# the command existed answers with its usage text: then the older commands do what they can. The
# error preference is lowered for the call: its complaints on stderr are text to read here, not errors.
$previous = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    $answer = (& $program uninstall @flags 2>&1 | Out-String)
    $code = $LASTEXITCODE
}
finally {
    $ErrorActionPreference = $previous
}
Write-Output $answer.TrimEnd()
if ($code -ne 0) {
    if ($answer -match 'usage:') {
        Write-Output 'this program is older than the uninstall command: stopping it and taking away the login entry the older way'
        & $program stop
        if ($LASTEXITCODE -ne 0) { Exit-Uninstall "hub-desktop stop failed (see above): the program stays in $dir" }
        & $program autostart off
        if ($LASTEXITCODE -ne 0) { Exit-Uninstall "hub-desktop autostart off failed (see above): the program stays in $dir" }
        Write-Output '(a link or launcher it made, and its settings, are not touched by an older program: delete them by hand)'
    }
    else {
        Exit-Uninstall "hub-desktop uninstall failed (see above): the program stays in $dir"
    }
}

# Only the files the installer puts there; a folder that holds anything else stays. A file that
# cannot be removed (a program that still runs from it) is an error, not a "removed" (rule RS-07).
foreach ($name in 'hub-desktop.exe', 'hub-desktop.exe.new') {
    $file = Join-Path $dir $name
    if (Test-Path -LiteralPath $file) {
        try { Remove-Item -LiteralPath $file -Force }
        catch { Stop-Uninstall "cannot remove ${file}: $($_.Exception.Message)" }
        if (Test-Path -LiteralPath $file) { Stop-Uninstall "$file is still there" }
    }
}
if (-not (Get-ChildItem -LiteralPath $dir -Force -ErrorAction SilentlyContinue)) {
    Remove-Item -LiteralPath $dir -Force -ErrorAction SilentlyContinue
    # The parent only when it is the folder the installer makes (never a folder you chose).
    $parent = Split-Path -Parent $dir
    if ((Split-Path -Leaf $dir) -eq 'bin' -and (Split-Path -Leaf $parent) -eq 'hub-desktop') {
        if (-not (Get-ChildItem -LiteralPath $parent -Force -ErrorAction SilentlyContinue)) {
            Remove-Item -LiteralPath $parent -Force -ErrorAction SilentlyContinue
        }
    }
}

Write-Output ''
Write-Output "removed $program"
if (-not $PurgeSettings) {
    Write-Output 'your settings were kept (-PurgeSettings removes them)'
}
