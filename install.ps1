# Installs aka on Windows.
#
#   irm https://github.com/zeuslcf/aka/releases/latest/download/install.ps1 | iex
#
# Settings (all optional):
#   $env:AKA_VERSION       version to install, like 0.1.0 (default: the latest)
#   $env:AKA_INSTALL_DIR   where aka.exe goes (default: %LOCALAPPDATA%\Programs\aka)

$ErrorActionPreference = 'Stop'
$repo = 'zeuslcf/aka'

$installDir = if ($env:AKA_INSTALL_DIR) { $env:AKA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\aka' }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { throw "aka install: no prebuilt binary for $($env:PROCESSOR_ARCHITECTURE)" }
}

$version = $env:AKA_VERSION
if (-not $version) {
    $latest = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $version = $latest.tag_name
}
$version = $version.TrimStart('v')

$name = "aka-v$version-$arch-pc-windows-msvc"
$base = if ($env:AKA_DOWNLOAD_BASE) { $env:AKA_DOWNLOAD_BASE } else { "https://github.com/$repo/releases/download/v$version" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory $tmp | Out-Null
try {
    Write-Host "Downloading aka $version for $arch..."
    Invoke-WebRequest "$base/$name.zip" -OutFile "$tmp\$name.zip" -UseBasicParsing
    Invoke-WebRequest "$base/$name.zip.sha256" -OutFile "$tmp\$name.zip.sha256" -UseBasicParsing

    $expected = ((Get-Content "$tmp\$name.zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash "$tmp\$name.zip" -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "aka install: checksum mismatch, the download may be corrupted (expected $expected, got $actual)"
    }

    Expand-Archive "$tmp\$name.zip" -DestinationPath $tmp -Force
    New-Item -ItemType Directory $installDir -Force | Out-Null
    Copy-Item "$tmp\$name\aka.exe" (Join-Path $installDir 'aka.exe') -Force
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to your PATH."
}

Write-Host "Installed aka $version to $installDir\aka.exe"
Write-Host "Next, run: aka setup"
