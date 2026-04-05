# Addness CLI installer for Windows
# Usage: irm https://cli.addness.com/install.ps1 | iex

$ErrorActionPreference = "Stop"

$CdnBase = if ($env:ADDNESS_CDN_BASE) { $env:ADDNESS_CDN_BASE } else { "https://cli.addness.com" }
$InstallDir = if ($env:ADDNESS_INSTALL_DIR) { $env:ADDNESS_INSTALL_DIR } else { "$env:LOCALAPPDATA\addness\bin" }
$Version = if ($env:ADDNESS_VERSION) { $env:ADDNESS_VERSION } else { "latest" }

function Write-Banner {
    $esc = [char]27

    Write-Host ""
    Write-Host "  ${esc}[1;34m                                        ."
    Write-Host "  ${esc}[1;34m                   .:=+*###***+=:.    =:"
    Write-Host "  ${esc}[1;34m               .=*%@@%*=:.    .:=**+#="
    Write-Host "  ${esc}[1;34m            .:*@@@@*:.            :#%*:"
    Write-Host "  ${esc}[1;34m          .+@@@@@*.            :+%%=. .+="
    Write-Host "  ${esc}[1;34m         =@@@@@@:          .=*%%+.     ::"
    Write-Host "  ${esc}[1;34m       .*@@@@@@.      .:+*%%%#=.        :"
    Write-Host "  ${esc}[1;34m      .@@@@@@@:  =+*#%%%%%%+:"
    Write-Host "  ${esc}[1;34m     .@@@@@@@+ .*%%%%%%#+:"
    Write-Host "  ${esc}[1;34m    .@@@@@@@@. *%%%%*=."
    Write-Host "  ${esc}[1;34m    *@@@@@@@+ .%%*=."
    Write-Host "  ${esc}[1;34m   :@@@@@@@@."
    Write-Host "  ${esc}[1;34m   #@@@@@@@*"
    Write-Host "  ${esc}[1;34m   ++==::..${esc}[0m"
    Write-Host ""
    Write-Host "  ${esc}[1m _         _            _       _     _                       _ _   _ "
    Write-Host "  | |    ___| |_ ___     / \   __| | __| |_ __   ___  ___ ___  (_) |_| |"
    Write-Host "  | |   / _ \ __/ __|   / _ \ / _`` |/ _`` | '_ \ / _ \/ __/ __| | | __| |"
    Write-Host "  | |__|  __/ |_\__ \  / ___ \ (_| | (_| | | | |  __/\__ \__ \ | | |_|_|"
    Write-Host "  |_____\___|\__|___/ /_/   \_\__,_|\__,_|_| |_|\___||___/___/ |_|\__(_)${esc}[0m"
    Write-Host ""
}

function Write-Step {
    param([string]$Message)
    Write-Host -NoNewline "  > ${Message}..."
}

function Write-StepOk {
    Write-Host " done" -ForegroundColor Green
}

function Write-Info {
    param([string]$Message)
    Write-Host "  > ${Message}"
}

function Write-Err {
    param([string]$Message)
    Write-Host "  ! ${Message}" -ForegroundColor Red
}

function Main {
    Write-Banner

    $Target = "x86_64-pc-windows-msvc"
    Write-Info "Platform  $Target"
    Write-Info "Version   $Version"
    Write-Host ""

    $BaseUrl = "${CdnBase}/releases/${Version}"
    $Archive = "addness-${Target}.zip"
    $Url = "${BaseUrl}/${Archive}"
    $ShaUrl = "${Url}.sha256"

    $TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "addness-install-$([System.Guid]::NewGuid())"
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

    try {
        Write-Step "Downloading"
        Invoke-WebRequest -Uri $Url -OutFile (Join-Path $TmpDir $Archive) -UseBasicParsing
        Invoke-WebRequest -Uri $ShaUrl -OutFile (Join-Path $TmpDir "${Archive}.sha256") -UseBasicParsing
        Write-StepOk

        Write-Step "Verifying checksum"
        $expectedLine = (Get-Content (Join-Path $TmpDir "${Archive}.sha256") -Raw).Trim()
        $expectedHash = ($expectedLine -split "\s+")[0].ToLower()
        $actualHash = (Get-FileHash -Algorithm SHA256 (Join-Path $TmpDir $Archive)).Hash.ToLower()
        if ($actualHash -ne $expectedHash) {
            Write-Host " failed" -ForegroundColor Red
            Write-Err "Checksum mismatch! Expected: ${expectedHash}, Got: ${actualHash}"
            exit 1
        }
        Write-StepOk

        Write-Step "Installing to ${InstallDir}"
        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }
        Expand-Archive -Path (Join-Path $TmpDir $Archive) -DestinationPath $TmpDir -Force
        Copy-Item (Join-Path $TmpDir "addness.exe") (Join-Path $InstallDir "addness.exe") -Force
        Write-StepOk

        # Add to PATH if not already present
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath -notlike "*${InstallDir}*") {
            Write-Step "Adding to PATH"
            [Environment]::SetEnvironmentVariable("Path", "${userPath};${InstallDir}", "User")
            $env:Path = "${env:Path};${InstallDir}"
            Write-StepOk
        }

        Write-Host ""
        $addnessPath = Join-Path $InstallDir "addness.exe"
        if (Test-Path $addnessPath) {
            $installedVersion = & $addnessPath --version 2>$null
            if ($installedVersion) {
                Write-Host "  * Addness CLI installed successfully! ${installedVersion}" -ForegroundColor Green
            } else {
                Write-Host "  * Installed to ${InstallDir}\addness.exe" -ForegroundColor Green
            }
        }

        Write-Host ""
        Write-Host "  Get started:"
        Write-Host "    addness login       Log in to your account"
        Write-Host "    addness goal list   View your goals"
        Write-Host ""
        Write-Host "  Note: Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
        Write-Host ""
    }
    finally {
        Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
    }
}

Main
