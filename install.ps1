[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$Dir = "$env:LOCALAPPDATA"
)

$ErrorActionPreference = "Stop"
$Dir = [IO.Path]::GetFullPath($Dir)
$Repo = "taokeqin/sessiontree"
$App = "Sessiontree"
$Asset = "sessiontree-windows-x86_64.zip"

if ($Version) {
    if (-not $Version.StartsWith("v")) { $Version = "v$Version" }
    $Base = "https://github.com/$Repo/releases/download/$Version"
} else {
    $Base = "https://github.com/$Repo/releases/latest/download"
}

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("sessiontree-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Tmp -Force | Out-Null

try {
    Write-Host "Downloading $App..."
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset" -OutFile (Join-Path $Tmp $Asset)
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS.txt" -OutFile (Join-Path $Tmp "SHA256SUMS.txt")

    $line = Select-String -Path (Join-Path $Tmp "SHA256SUMS.txt") -Pattern "\s$([regex]::Escape($Asset))$" | Select-Object -First 1
    if (-not $line) { throw "SHA256SUMS.txt does not list $Asset" }
    $Expected = ($line.Line -split "\s+")[0].ToLower()
    $Actual = (Get-FileHash -Path (Join-Path $Tmp $Asset) -Algorithm SHA256).Hash.ToLower()
    if ($Actual -ne $Expected) { throw "checksum mismatch for $Asset" }

    Expand-Archive -Path (Join-Path $Tmp $Asset) -DestinationPath $Tmp -Force
    $source = Join-Path $Tmp $App
    if (-not (Test-Path $source)) { throw "the release does not contain $App" }

    $dest = Join-Path $Dir $App
    if (Test-Path $dest) { Remove-Item -Path $dest -Recurse -Force }
    New-Item -ItemType Directory -Path $Dir -Force | Out-Null
    Move-Item -Path $source -Destination $dest

    $exe = Join-Path $dest "$App.exe"
    $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    New-Item -ItemType Directory -Path $startMenu -Force | Out-Null
    $shortcutPath = Join-Path $startMenu "$App.lnk"
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $exe
    $shortcut.WorkingDirectory = $dest
    $shortcut.Save()

    Write-Host "Installed $exe"
    Write-Host "Launch it from the Start menu, or run: $exe"
} finally {
    Remove-Item -Path $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}
