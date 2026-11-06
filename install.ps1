# Installs or updates splynt, the Splynt terminal app, on Windows.
#
#   irm https://raw.githubusercontent.com/elijahgronda/Splynt/main/install.ps1 | iex
#
# It puts splynt.exe in %LOCALAPPDATA%\Programs\splynt and adds that folder to
# your PATH. Set SPLYNT_INSTALL_DIR to use another folder.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'elijahgronda/Splynt'
$dir = if ($env:SPLYNT_INSTALL_DIR) { $env:SPLYNT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\splynt' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("splynt-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
    if ($env:SPLYNT_ZIP) {
        # A local zip, for testing the script against a build.
        $zip = $env:SPLYNT_ZIP
        $version = 'from ' + $zip
    } else {
        # The newest CLI release. Desktop releases are tagged v*, the CLI cli-v*.
        $releases = Invoke-RestMethod "https://api.github.com/repos/$repo/releases?per_page=50" -Headers @{ 'User-Agent' = 'splynt-install' }
        $release = $releases | Where-Object { $_.tag_name -like 'cli-v*' } | Select-Object -First 1
        if (-not $release) { throw "Couldn't find a Splynt CLI release on GitHub." }
        $version = $release.tag_name -replace '^cli-v', ''
        $zip = Join-Path $tmp 'splynt.zip'
        Write-Host "Downloading splynt $version..."
        Invoke-WebRequest "https://github.com/$repo/releases/download/$($release.tag_name)/splynt-windows-x64.zip" -OutFile $zip -UseBasicParsing
    }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Get-ChildItem -Path $tmp -Recurse -Filter 'splynt.exe' | Select-Object -First 1
    if (-not $exe) { throw "The download didn't have splynt.exe in it." }

    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $target = Join-Path $dir 'splynt.exe'
    try {
        Copy-Item $exe.FullName $target -Force
    } catch {
        throw "Couldn't replace $target. If splynt is open, quit it and run this again."
    }
    Unblock-File $target -ErrorAction SilentlyContinue
    Write-Host "Installed splynt $version to $target"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = if ($userPath) { $userPath -split ';' } else { @() }
    if ($parts -notcontains $dir) {
        $newPath = if ($userPath) { "$userPath;$dir" } else { $dir }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Write-Host "Added $dir to your PATH. Open a new terminal window, then run: splynt"
    } else {
        Write-Host "Run it with: splynt"
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
