[CmdletBinding()]
param(
    [string]$ReleaseDirectory = "",
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($ReleaseDirectory)) {
    $ReleaseDirectory = Join-Path $repoRoot "dist"
}
$ReleaseDirectory = [System.IO.Path]::GetFullPath($ReleaseDirectory)
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = (& cargo metadata --manifest-path (Join-Path $repoRoot "rust\Cargo.toml") --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
}
$cli = Join-Path $repoRoot "rust\target\release\zhfd-checkin.exe"
$gui = Join-Path $repoRoot "rust\target\release\zhfd-checkin-gui.exe"
foreach ($path in @($cli, $gui)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Release EXE not found: $path"
    }
}

$commit = (& git -C $repoRoot rev-parse --short=12 HEAD).Trim()
$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("zhfd-release-" + [guid]::NewGuid().ToString("N"))
$packageName = "zhfd-checkin-windows-x64-v$Version-$commit"
$packageDir = Join-Path $stage $packageName
$zip = Join-Path $ReleaseDirectory "$packageName.zip"

try {
    New-Item -ItemType Directory -Path $packageDir -Force | Out-Null
    Copy-Item -LiteralPath $cli -Destination (Join-Path $packageDir "zhfd-checkin.exe")
    Copy-Item -LiteralPath $gui -Destination (Join-Path $packageDir "zhfd-checkin-gui.exe")
    @"
智汇福大自动签到 Windows 便携版

版本：$Version
Git commit：$commit
默认模式：dry-run
签到窗口：21:00–23:59（Asia/Shanghai）

使用：
1. 双击 zhfd-checkin-gui.exe 打开 GUI；
2. 或运行 zhfd-checkin.exe about / instance list / run --dry-run；
3. 首次运行会在 EXE 同目录创建 config.toml、logs、captures、reports；
4. LDPlayer、ADB 和 App 登录状态由用户本机提供；
5. 本发布包不包含账号、Cookie、Token、配置、运行日志或截图；
6. live 运行必须由用户明确确认，并继续受 Profile、时间窗口、前台包名和 ready 门禁保护。

当前已验证：CLI/GUI parity、三实例隔离、ADB 重连、portable smoke。
计划任务默认建议使用 dry-run：
zhfd-checkin.exe task create --name ZHFD-AutoCheckin-DryRun --all-instances
"@ | Set-Content -LiteralPath (Join-Path $packageDir "README.txt") -Encoding utf8
    New-Item -ItemType Directory -Path $ReleaseDirectory -Force | Out-Null
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    Compress-Archive -LiteralPath $packageDir -DestinationPath $zip -CompressionLevel Optimal
    Write-Host "RELEASE_PACKAGE_PASS"
    Write-Host "package=$zip"
    Write-Host "commit=$commit"
    Write-Host "version=$Version"
}
finally {
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
}