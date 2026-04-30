# Gforce Node Agent installer — Windows (PowerShell).
#
# One-shot usage (auto-register + install service):
#   $env:TOKEN = "<enrollment-token>"
#   iwr https://gforce.nearminds.org/install.ps1 -useb | iex
#
# Manual usage:
#   iwr https://gforce.nearminds.org/install.ps1 -useb | iex
#   # then, in an elevated shell:
#   gforce-node register --token <token> --server gforce.nearminds.org
#   gforce-node install
#
# Env vars:
#   GFORCE_INSTALL_DIR   install target (default: C:\Program Files\Gforce)
#   GFORCE_VERSION       release tag (default: latest)
#   GFORCE_SERVER        server hostname (default: gforce.nearminds.org)
#   TOKEN                one-time enrollment token — if set, we auto-run
#                        register + install.
#   GFORCE_NO_SERVICE    set to "1" to skip service install.

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:GFORCE_INSTALL_DIR) { $env:GFORCE_INSTALL_DIR } else { "$env:ProgramFiles\Gforce" }
$Repo       = "nearminds/GforceNode"
$Version    = if ($env:GFORCE_VERSION) { $env:GFORCE_VERSION } else { "latest" }
$Server     = if ($env:GFORCE_SERVER) { $env:GFORCE_SERVER } else { "gforce.nearminds.org" }

function Detect-Arch {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { return "x86_64" }
        "ARM64" { return "aarch64" }
        default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

function Get-DownloadUrl {
    param($Platform)
    if ($Version -eq "latest") {
        return "https://github.com/$Repo/releases/latest/download/gforce-node-$Platform.zip"
    } else {
        return "https://github.com/$Repo/releases/download/$Version/gforce-node-$Platform.zip"
    }
}

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = New-Object Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

Write-Host "Gforce Node Agent installer (Windows)"
Write-Host "====================================="
Write-Host ""

$arch     = Detect-Arch
$platform = "windows-$arch"
Write-Host "Platform: $platform"

$url = Get-DownloadUrl -Platform $platform
Write-Host "Download: $url"
Write-Host ""

$tmp = Join-Path $env:TEMP "gforce-node-install-$(Get-Random)"
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $archive = Join-Path $tmp "gforce-node.zip"
    Write-Host "Downloading..."
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing

    # Sanity-check the size before handing the file to Expand-Archive.
    # GitHub returns a tiny error page when an asset doesn't exist, and
    # the user gets an opaque "not a valid zip" later. The smallest real
    # zip we ship is several MB; 1 KB is a generous floor.
    $size = (Get-Item $archive).Length
    if ($size -lt 1024) {
        $head = [System.IO.File]::ReadAllText($archive)
        throw "Downloaded archive is only $size bytes — that's not a real zip. Body: $($head.Substring(0, [Math]::Min(200, $head.Length)))"
    }

    # SHA256 verification — same rationale as install.sh. Skipping is
    # supported via $env:GFORCE_SKIP_VERIFY = "1" but loud.
    if ($env:GFORCE_SKIP_VERIFY -eq "1") {
        Write-Host "Skipping SHA256 verification (GFORCE_SKIP_VERIFY=1)."
    } else {
        $sumsUrl = if ($Version -eq "latest") {
            "https://github.com/$Repo/releases/latest/download/sha256sums.txt"
        } else {
            "https://github.com/$Repo/releases/download/$Version/sha256sums.txt"
        }
        $sumsFile = Join-Path $tmp "sha256sums.txt"
        Write-Host "Verifying SHA256..."
        try {
            Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile -UseBasicParsing
        } catch {
            throw "sha256sums.txt could not be fetched from $sumsUrl. Either this release predates checksum support (re-cut it), or set `$env:GFORCE_SKIP_VERIFY = '1' to skip."
        }
        $expected = (Select-String -Path $sumsFile -Pattern "gforce-node-$platform.zip" |
            Select-Object -First 1).Line.Split()[0]
        if (-not $expected) {
            throw "No checksum line for gforce-node-$platform.zip in sha256sums.txt"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
        if ($actual -ne $expected.ToLower()) {
            throw "SHA256 mismatch. expected=$expected actual=$actual"
        }
        Write-Host "Checksum OK ($actual)."
    }

    Write-Host "Extracting..."
    Expand-Archive -Path $archive -DestinationPath $tmp -Force

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir | Out-Null
    }

    $cli    = Get-ChildItem -Path $tmp -Filter "gforce-node.exe" -Recurse | Select-Object -First 1
    $daemon = Get-ChildItem -Path $tmp -Filter "gforce-node-daemon.exe" -Recurse | Select-Object -First 1
    if (-not $cli -or -not $daemon) {
        throw "Release archive missing gforce-node.exe or gforce-node-daemon.exe"
    }

    Copy-Item $cli.FullName    (Join-Path $InstallDir "gforce-node.exe") -Force
    Copy-Item $daemon.FullName (Join-Path $InstallDir "gforce-node-daemon.exe") -Force

    # Put the CLI on the user PATH so subsequent shells can find it.
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable(
            "PATH", ($userPath.TrimEnd(";") + ";$InstallDir"), "User"
        )
        Write-Host "Added $InstallDir to user PATH (open a new shell to pick it up)."
    }

    Write-Host ""
    Write-Host "Binaries installed to $InstallDir."

    if ($env:TOKEN) {
        if (-not (Test-Admin)) {
            Write-Warning "Service install requires an elevated (Administrator) PowerShell."
            Write-Host "Binary install is complete. Re-run this script in an elevated shell"
            Write-Host "with TOKEN set to finish enrolment and install the service."
            exit 0
        }

        $cliExe = Join-Path $InstallDir "gforce-node.exe"

        Write-Host ""
        Write-Host "Registering this machine with Gforce (server: $Server)..."
        & $cliExe register --token $env:TOKEN --server $Server
        if ($LASTEXITCODE -ne 0) { throw "Registration failed" }

        if ($env:GFORCE_NO_SERVICE -ne "1") {
            Write-Host ""
            Write-Host "Installing Windows Service..."
            & $cliExe install
            if ($LASTEXITCODE -ne 0) { throw "Service install failed" }
        } else {
            Write-Host "Skipping service install (GFORCE_NO_SERVICE=1)."
        }

        Write-Host ""
        Write-Host "Done. Check status:"
        Write-Host "  gforce-node status"
    } else {
        Write-Host ""
        Write-Host "Next steps (in an elevated PowerShell):"
        Write-Host "  gforce-node register --token <TOKEN> --server $Server"
        Write-Host "  gforce-node install"
        Write-Host ""
        Write-Host "Tip: set `$env:TOKEN='<token>'` before running this script"
        Write-Host "     to do both in one command."
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
