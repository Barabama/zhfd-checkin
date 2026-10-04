[CmdletBinding()]
param(
    [string]$ExePath = "",
    [string]$FixturePath = "",
    [switch]$KeepStaging
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $ExePath = Join-Path $repoRoot "rust\target\release\zhfd-checkin.exe"
}
if ([string]::IsNullOrWhiteSpace($FixturePath)) {
    $FixturePath = Join-Path $repoRoot "autojs6\fixtures\ready.png"
}

$ExePath = (Resolve-Path -LiteralPath $ExePath).Path
$FixturePath = (Resolve-Path -LiteralPath $FixturePath).Path
if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    throw "Release EXE not found: $ExePath"
}
if (-not (Test-Path -LiteralPath $FixturePath -PathType Leaf)) {
    throw "Fixture not found: $FixturePath"
}

$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("zhfd-portable-smoke-" + [guid]::NewGuid().ToString("N"))
$stageExe = Join-Path $stage "zhfd-checkin.exe"
$stageGuiExe = Join-Path $stage "zhfd-checkin-gui.exe"
$stageFixtureDir = Join-Path $stage "fixtures"
$stageFixture = Join-Path $stageFixtureDir "ready.png"

function Invoke-PortableApp {
    param([string[]]$AppArgs)
    Push-Location -LiteralPath $stage
    try {
        $output = (& $stageExe @AppArgs 2>&1 | Out-String).Trim()
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    if ($exitCode -ne 0) {
        throw "Portable command failed (exit=$exitCode): zhfd-checkin.exe $($AppArgs -join ' ')`n$output"
    }
    return $output
}

try {
    New-Item -ItemType Directory -Path $stageFixtureDir -Force | Out-Null
    Copy-Item -LiteralPath $ExePath -Destination $stageExe
    $guiExePath = Join-Path (Split-Path -Parent $ExePath) "zhfd-checkin-gui.exe"
    if (-not (Test-Path -LiteralPath $guiExePath -PathType Leaf)) {
        throw "Standalone GUI EXE not found beside CLI EXE: $guiExePath"
    }
    Copy-Item -LiteralPath $guiExePath -Destination $stageGuiExe
    Copy-Item -LiteralPath $FixturePath -Destination $stageFixture

    $about = Invoke-PortableApp @("about")
    if ($about -notmatch "zhfd-checkin") {
        throw "Portable about output did not contain the application name."
    }
    foreach ($aboutMarker in @("Git commit:", "支持 Profile:", "视觉依赖:", "设备依赖:")) {
        if ($about -notmatch [regex]::Escape($aboutMarker)) {
            throw "Portable about output is missing metadata: $aboutMarker"
        }
    }

    $profiles = Invoke-PortableApp @("profile", "list")
    foreach ($id in @(
        "landscape_1600x900_d240",
        "portrait_900x1600_d240",
        "landscape_1280x720_d240",
        "portrait_720x1280_d240",
        "landscape_1920x1080_d280",
        "portrait_1080x1920_d280",
        "landscape_960x540_d160",
        "portrait_540x960_d160"
    )) {
        if ($profiles -notmatch [regex]::Escape($id)) {
            throw "Portable profile list is missing: $id"
        }
    }

    $config = Invoke-PortableApp @("config", "show")
    $stageConfig = Join-Path $stage "config.toml"
    if (-not (Test-Path -LiteralPath $stageConfig -PathType Leaf)) {
        throw "Portable config.toml was not created beside the EXE."
    }
    if ($config -notmatch [regex]::Escape($stageConfig)) {
        throw "Portable config output did not resolve beside the EXE: $stageConfig"
    }

    foreach ($directoryName in @("logs", "captures", "reports")) {
        $stageDirectory = Join-Path $stage $directoryName
        if (-not (Test-Path -LiteralPath $stageDirectory -PathType Container)) {
            throw "Portable EXE did not create expected directory: $directoryName"
        }
    }

    $reportText = Invoke-PortableApp @("report")
    $stageReports = Join-Path $stage "reports"
    if (-not (Test-Path -LiteralPath $stageReports -PathType Container)) {
        throw "Portable report command did not create reports directory."
    }
    if ($reportText -notmatch "诊断报告") {
        throw "Portable report command did not print a report path."
    }

    $visionText = Invoke-PortableApp @("vision", "--image", (Join-Path "fixtures" "ready.png"))
    $vision = $visionText | ConvertFrom-Json
    if ($vision.state -ne "ready") {
        throw "Portable vision smoke expected ready, got: $($vision.state)"
    }
    if ([int]$vision.white_count -le 0) {
        throw "Portable vision smoke returned no white text pixels."
    }

    Write-Host "PORTABLE_SMOKE_PASS"
    Write-Host "staging=$stage"
    Write-Host "exe=$stageExe"
    Write-Host "gui_exe=$stageGuiExe"
    Write-Host "profile_count=8+"
    Write-Host "vision_state=$($vision.state)"
    Write-Host "runtime_dirs=logs,captures,reports"
    Write-Host "config=$stageConfig"
}
finally {
    if ($KeepStaging) {
        Write-Host "staging_kept=$stage"
    }
    elseif (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
}
