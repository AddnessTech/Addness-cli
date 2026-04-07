# Addness CLI installer for Windows
# Usage: irm https://cli.addness.com/install.ps1 | iex

$ErrorActionPreference = "Stop"

$CdnBase = if ($env:ADDNESS_CDN_BASE) { $env:ADDNESS_CDN_BASE } else { "https://cli.addness.com" }
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
        # ダウンロード（プログレス表示）
        $archivePath = Join-Path $TmpDir $Archive
        Write-Host -NoNewline "  > Downloading...  0%"
        $webClient = New-Object System.Net.WebClient
        $downloadComplete = $false
        $eventHandler = Register-ObjectEvent -InputObject $webClient -EventName DownloadProgressChanged -Action {
            $pct = $EventArgs.ProgressPercentage
            Write-Host -NoNewline "`r  > Downloading...  ${pct}%  "
        }
        $completedHandler = Register-ObjectEvent -InputObject $webClient -EventName DownloadFileCompleted -Action {
            $script:downloadComplete = $true
        }
        $webClient.DownloadFileAsync([Uri]$Url, $archivePath)
        while (-not $downloadComplete) { Start-Sleep -Milliseconds 100 }
        Unregister-Event -SourceIdentifier $eventHandler.Name
        Unregister-Event -SourceIdentifier $completedHandler.Name
        $webClient.Dispose()
        Write-Host "`r  > Downloading...  " -NoNewline
        Write-Host "100%" -ForegroundColor Green
        Invoke-WebRequest -Uri $ShaUrl -OutFile (Join-Path $TmpDir "${Archive}.sha256") -UseBasicParsing

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

        # Clean up legacy install location
        $LegacyBin = Join-Path $env:USERPROFILE ".addness\bin\addness.exe"
        $CurrentBin = Join-Path $InstallDir "addness.exe"
        if ((Test-Path $LegacyBin) -and ($LegacyBin -ne $CurrentBin)) {
            Write-Step "Removing old binary at $LegacyBin"
            Remove-Item -Force $LegacyBin -ErrorAction SilentlyContinue
            Write-StepOk
        }

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
