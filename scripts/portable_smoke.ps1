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
$previousLocalAppData = $env:LOCALAPPDATA

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

function Invoke-PortableAppExpectFailure {
    param([string[]]$AppArgs)
    $stdoutPath = Join-Path $stage ("smoke-stdout-" + [guid]::NewGuid().ToString("N") + ".txt")
    $stderrPath = Join-Path $stage ("smoke-stderr-" + [guid]::NewGuid().ToString("N") + ".txt")
    Push-Location -LiteralPath $stage
    try {
        & $stageExe @AppArgs 1> $stdoutPath 2> $stderrPath
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    $output = if (Test-Path -LiteralPath $stdoutPath) {
        Get-Content -LiteralPath $stdoutPath -Raw
    } else {
        ""
    }
    $errorOutput = if (Test-Path -LiteralPath $stderrPath) {
        Get-Content -LiteralPath $stderrPath -Raw
    } else {
        ""
    }
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
    if ($exitCode -eq 0) {
        throw "Portable command unexpectedly succeeded: zhfd-checkin.exe $($AppArgs -join ' ')`n$output`n$errorOutput"
    }
    $safeOutput = if ($null -eq $output) { "" } else { $output.ToString().Trim() }
    $safeErrorOutput = if ($null -eq $errorOutput) { "" } else { $errorOutput.ToString().Trim() }
    return [pscustomobject]@{
        ExitCode = $exitCode
        Output = $safeOutput
        ErrorOutput = $safeErrorOutput
    }
}

try {
    New-Item -ItemType Directory -Path $stageFixtureDir -Force | Out-Null
    $env:LOCALAPPDATA = Join-Path $stage "local-app-data"
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
        "portrait_720x1280_d320",
        "landscape_1920x1080_d280",
        "portrait_1080x1920_d480",
        "landscape_960x540_d160",
        "portrait_540x960_d240",
        "portrait_900x1600_d320"
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

    $configPath = Invoke-PortableApp @("--config", $stageConfig, "config", "path")
    foreach ($marker in @("path=$stageConfig", "origin=explicit", "exists=true")) {
        if ($configPath -notmatch [regex]::Escape($marker)) {
            throw "Explicit config path output is missing: $marker`n$configPath"
        }
    }

    Set-Content -LiteralPath (Join-Path $stage "ldconsole.exe") -Value "Not a Windows executable."
    $originalConfig = Get-Content -LiteralPath $stageConfig -Raw
    $offlineConfig = $originalConfig -replace '(?m)^auto_launch = true\s*$', 'auto_launch = false'
    Set-Content -LiteralPath $stageConfig -Value $offlineConfig -NoNewline
    $dryRun = Invoke-PortableAppExpectFailure @(
        "--config", $stageConfig, "run", "--dry-run", "--json", "--instance-index", "0"
    )
    try {
        $dryRunResult = $dryRun.Output | ConvertFrom-Json
    }
    catch {
        throw "Dry-run failure did not produce valid JSON: $($dryRun.Output)"
    }
    if ($dryRunResult.clicked -ne $false) {
        throw "Dry-run failure must report clicked=false."
    }
    if ($dryRun.ExitCode -ne 2 -or $dryRunResult.error_code -ne "device_setup_failed") {
        throw "Dry-run did not report the expected device setup failure."
    }
    $resultPath = Join-Path $stage "logs/$($dryRunResult.run_id)/result.json"
    $savedResult = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if ($savedResult.clicked -ne $false -or $savedResult.error_code -ne $dryRunResult.error_code) {
        throw "Saved dry-run result disagrees with the CLI output."
    }

    Set-Content -LiteralPath $stageConfig -Value "not valid toml" -NoNewline
    $brokenConfig = Invoke-PortableAppExpectFailure @("--config", $stageConfig, "config", "show")
    if (($brokenConfig.Output + "`n" + $brokenConfig.ErrorOutput) -notmatch "不会自动恢复默认配置") {
        throw "Broken config output did not explain that the file is preserved."
    }
    if ((Get-Content -LiteralPath $stageConfig -Raw) -ne "not valid toml") {
        throw "Broken config was silently overwritten."
    }
    Set-Content -LiteralPath $stageConfig -Value $originalConfig -NoNewline

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
    $env:LOCALAPPDATA = $previousLocalAppData
    if ($KeepStaging) {
        Write-Host "staging_kept=$stage"
    }
    elseif (Test-Path -LiteralPath $stage) {
        $resolvedStage = (Resolve-Path -LiteralPath $stage).Path
        $tempRoot = (Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath())).Path.TrimEnd('\', '/')
        $stageName = Split-Path -Leaf $resolvedStage
        if ((Split-Path -Parent $resolvedStage) -ne $tempRoot -or $stageName -notmatch '^zhfd-portable-smoke-[0-9a-f]{32}$') {
            throw "Refusing to remove unexpected staging directory: $resolvedStage"
        }
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
}
