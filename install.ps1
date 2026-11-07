# Addness CLI installer for Windows
# Usage: irm https://d22sm2skny3y0p.cloudfront.net/install.ps1 | iex

$ErrorActionPreference = "Stop"

$CdnBase = if ($env:ADDNESS_CDN_BASE) { $env:ADDNESS_CDN_BASE } else { "https://d22sm2skny3y0p.cloudfront.net" }
$InstallDir = if ($env:ADDNESS_INSTALL_DIR) { $env:ADDNESS_INSTALL_DIR } else { "$env:LOCALAPPDATA\addness\bin" }
$Version = if ($env:ADDNESS_VERSION) { $env:ADDNESS_VERSION } else { "latest" }

function Write-Banner {
    $logo = @(
        "                                            ."
        "                       .:=+*###***+=:.    =:"
        "                   .=*%@@%*=:.    .:=**+#="
        "                .:*@@@@*:.            :#%*:"
        "              .+@@@@@*.            :+%%=. .+="
        "             =@@@@@@:          .=*%%+.     ::"
        "           .*@@@@@@.      .:+*%%%#=.        :"
        "          .@@@@@@@:  =+*#%%%%%%+:"
        "         .@@@@@@@+ .*%%%%%%#+:"
        "        .@@@@@@@@. *%%%%*=."
        "        *@@@@@@@+ .%%*=."
        "       :@@@@@@@@."
        "       #@@@@@@@*"
        "       ++==::..`n"
    )
    $text = @(
        "   _         _            _       _     _                       _ _   _ "
        "  | |    ___| |_ ___     / \   __| | __| |_ __   ___  ___ ___  (_) |_| |"
        "  | |   / _ \ __/ __|   / _ \ / _`` |/ _`` | '_ \ / _ \/ __/ __| | | __| |"
        "  | |__|  __/ |_\__ \  / ___ \ (_| | (_| | | | |  __/\__ \__ \ | | |_|_|"
        "  |_____\___|\__|___/ /_/   \_\__,_|\__,_|_| |_|\___||___/___/ |_|\__(_)"
    )

    Write-Host ""
    foreach ($line in $logo) {
        Write-Host $line -ForegroundColor Blue
        Start-Sleep -Milliseconds 30
    }
    foreach ($line in $text) {
        Write-Host $line -ForegroundColor White
        Start-Sleep -Milliseconds 30
    }
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

    if ($Version -eq "latest") {
        $Version = (Invoke-RestMethod -Uri "${CdnBase}/releases/latest/version.txt" -TimeoutSec 30).Trim()
    }
    if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') {
        throw "Invalid release version: ${Version}"
    }

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
        # ダウンロード
        $archivePath = Join-Path $TmpDir $Archive
        Write-Step "Downloading"
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $Url -OutFile $archivePath -UseBasicParsing -TimeoutSec 120
        Invoke-WebRequest -Uri $ShaUrl -OutFile (Join-Path $TmpDir "${Archive}.sha256") -UseBasicParsing -TimeoutSec 30
        $ProgressPreference = 'Continue'
        Write-StepOk

        Write-Step "Verifying checksum"
        $expectedLine = (Get-Content (Join-Path $TmpDir "${Archive}.sha256") -Raw).Trim()
        $checksumParts = $expectedLine -split "\s+"
        if ($checksumParts.Count -ne 2 -or $checksumParts[0] -notmatch '^[a-fA-F0-9]{64}$' -or $checksumParts[1] -cne $Archive) {
            throw "Invalid checksum record for ${Archive}"
        }
        $expectedHash = $checksumParts[0].ToLower()
        $actualHash = (Get-FileHash -Algorithm SHA256 (Join-Path $TmpDir $Archive)).Hash.ToLower()
        if ($actualHash -ne $expectedHash) {
            Write-Host " failed" -ForegroundColor Red
            Write-Err "Checksum mismatch! Expected: ${expectedHash}, Got: ${actualHash}"
            exit 1
        }
        Write-StepOk

        Expand-Archive -Path (Join-Path $TmpDir $Archive) -DestinationPath $TmpDir -Force
        $downloadedVersion = & (Join-Path $TmpDir "addness.exe") --version
        if ($LASTEXITCODE -ne 0 -or $downloadedVersion -cne "addness $($Version.Substring(1))") {
            throw "Downloaded binary version mismatch: expected addness $($Version.Substring(1)), got ${downloadedVersion}"
        }

        Write-Step "Installing to ${InstallDir}"
        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }
        $CurrentBin = Join-Path $InstallDir "addness.exe"
        # Windows cannot overwrite a running executable. Rename it first, and
        # restore it if placing the verified replacement fails.
        Get-ChildItem -LiteralPath $InstallDir -Filter 'addness.exe.old-*' -File |
            Remove-Item -Force -ErrorAction SilentlyContinue
        $BackupBin = Join-Path $InstallDir "addness.exe.old-$([System.Guid]::NewGuid())"
        $StagedBin = Join-Path $InstallDir ".addness-new-$([System.Guid]::NewGuid()).exe"
        Copy-Item (Join-Path $TmpDir "addness.exe") $StagedBin
        try {
            if (Test-Path -LiteralPath $CurrentBin) {
                Move-Item -LiteralPath $CurrentBin -Destination $BackupBin
            }
            try {
                Move-Item -LiteralPath $StagedBin -Destination $CurrentBin
            } catch {
                if (Test-Path -LiteralPath $BackupBin) {
                    Move-Item -LiteralPath $BackupBin -Destination $CurrentBin
                }
                throw
            }
        } finally {
            Remove-Item -LiteralPath $StagedBin -Force -ErrorAction SilentlyContinue
        }
        Remove-Item -LiteralPath $BackupBin -Force -ErrorAction SilentlyContinue
        Write-StepOk

        # Clean up legacy install location
        $LegacyBin = Join-Path $env:USERPROFILE ".addness\bin\addness.exe"
        $CurrentBin = Join-Path $InstallDir "addness.exe"
        if ($InstallDir -eq "$env:LOCALAPPDATA\addness\bin" -and (Test-Path $LegacyBin)) {
            Write-Step "Removing old binary at $LegacyBin"
            Remove-Item -Force $LegacyBin -ErrorAction SilentlyContinue
            Write-StepOk
        }

        # Add to PATH if not already present
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($env:ADDNESS_NO_MODIFY_PATH -ne '1' -and $userPath -notlike "*${InstallDir}*") {
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
        Write-Host "  AI integration:"
        Write-Host "    addness skills      Output AI skills prompt"
        Write-Host "    addness skills >> CLAUDE.md  Add to your project" -ForegroundColor DarkGray
        Write-Host ""
        Write-Host "  Note: Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
        Write-Host ""
    }
    finally {
        Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
    }
}

Main
