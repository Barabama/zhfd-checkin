[CmdletBinding()]
param(
    [string]$WorkRoot = "",
    [switch]$KeepScratch,
    [switch]$CheckRealTag
)

# Offline smoke test for the release gates in .github/workflows/release.yml.
#
# The point of this script is that it exercises the guards that actually ship:
# it extracts the `run:` block of each guard step straight out of the workflow
# file rather than re-implementing the checks, so editing a guard changes what
# is tested here automatically. Three guards are covered:
#
#   1. "Verify the release ref is a tag bound to the Cargo version"  (pre-build)
#   2. "Verify the packaged asset matches the tag"                   (post-package)
#   3. "Re-verify the tag and any existing Release before publishing" (pre-publish)
#
# Everything runs in a synthetic world built under $WorkRoot - a bare "origin"
# repository, a seed repo and one scratch clone per scenario. The scenarios
# never push, tag or publish, and they never write to this repository.
#
# The pre-publish guard asks the GitHub API whether a Release already exists.
# Reaching the real API would need a token and network, so the scenarios put a
# compiled stand-in (scripts/GhStub.cs, built with csc.exe) on PATH as `gh` and
# feed it the answer each scenario wants. A .cmd shim was tried first and
# rejected: cmd.exe runs AutoRun hooks (a conda hook on the author's machine),
# and their output lands in the captured stdout and corrupts the JSON.
#
# What it DOES touch in the real repository: it reads `rust/Cargo.toml` for the
# version, so the synthetic fixture is built with the version the guards will
# accept, and it reads `release.yml` for the guard source. Both are reads.
#
# -CheckRealTag additionally runs the first guard against this checkout when
# HEAD is exactly the released tag (the state a tag push produces). That case
# runs the guard's `git fetch` against the real `origin`, which writes
# `refs/remotes/origin/<default>` and `FETCH_HEAD` into the real `.git`. It is
# therefore opt-in rather than part of the default run.
#
# Usage:
#   .\scripts\simulate_release_guards.ps1
#   .\scripts\simulate_release_guards.ps1 -KeepScratch
#   .\scripts\simulate_release_guards.ps1 -CheckRealTag
#
# Requires Windows PowerShell 7 (`pwsh`) and csc.exe from .NET Framework 4.x.
# Exit code 0 = every scenario behaved as expected, 1 = at least one mismatch.

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$workflowPath = Join-Path $repoRoot ".github\workflows\release.yml"
if (-not (Test-Path -LiteralPath $workflowPath -PathType Leaf)) {
    throw "Release workflow not found: $workflowPath"
}

if ([string]::IsNullOrWhiteSpace($WorkRoot)) {
    $WorkRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("zhfd-guard-sim-" + [guid]::NewGuid().ToString("N"))
}
$WorkRoot = [System.IO.Path]::GetFullPath($WorkRoot)
if (Test-Path -LiteralPath $WorkRoot) { Remove-Item -LiteralPath $WorkRoot -Recurse -Force }
New-Item -ItemType Directory -Path $WorkRoot -Force | Out-Null

# The guards read `rust/Cargo.toml` from the checked-out tree to decide which
# tag they will accept, so the synthetic fixture is built with the project's
# real version. Reading it here (rather than hard-coding a test version) keeps
# scenario A meaningful after a version bump and stops a bump from looking like
# a bug in the guard under test.
$TEST_VERSION = (Select-String -LiteralPath (Join-Path $repoRoot "rust\Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$TEST_TAG = "v$TEST_VERSION"
$WRONG_TAG = if ($TEST_VERSION -eq "0.0.1") { "v0.0.2" } else { "v0.0.1" }
$DEFAULT_BRANCH = "master"
$GIT_IDENTITY = @("-c", "user.email=simulate@local", "-c", "user.name=simulate")

function Get-StepScript {
    <#
      Returns the `run:` block of the named step from a workflow file, so the
      tests run the same PowerShell that GitHub will run.
    #>
    param([string]$Path, [string]$StepName)
    $lines = Get-Content -LiteralPath $Path
    $start = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match ("^\s*-\s+name:\s*" + [regex]::Escape($StepName) + "\s*$")) { $start = $i; break }
    }
    if ($start -lt 0) { throw "Step not found in $Path`: $StepName" }
    $runIndex = -1
    $runIndent = 0
    for ($i = $start + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match "^(\s*)-\s") { break }
        if ($lines[$i] -match "^(\s*)run:\s*\|") { $runIndex = $i; $runIndent = $Matches[1].Length; break }
    }
    if ($runIndex -lt 0) { throw "Step '$StepName' has no 'run: |' block." }
    $body = @()
    for ($i = $runIndex + 1; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line.Trim().Length -eq 0) { $body += ""; continue }
        if (($line.Length - $line.TrimStart().Length) -le $runIndent) { break }
        $body += $line
    }
    if ($body.Count -eq 0) { throw "Step '$StepName' has an empty run block." }
    $strip = $body[0].Length - $body[0].TrimStart().Length
    ($body | ForEach-Object { if ($_.Length -ge $strip) { $_.Substring($strip) } else { $_.TrimStart() } }) -join "`n"
}

$guardScript = Join-Path $WorkRoot "release_guard.ps1"
Set-Content -LiteralPath $guardScript -Encoding utf8 -Value (Get-StepScript $workflowPath "Verify the release ref is a tag bound to the Cargo version")
$assetGuardScript = Join-Path $WorkRoot "asset_guard.ps1"
Set-Content -LiteralPath $assetGuardScript -Encoding utf8 -Value (Get-StepScript $workflowPath "Verify the packaged asset matches the tag")
$prepublishGuardScript = Join-Path $WorkRoot "prepublish_guard.ps1"
Set-Content -LiteralPath $prepublishGuardScript -Encoding utf8 -Value (Get-StepScript $workflowPath "Re-verify the tag and any existing Release before publishing")

# The guards are PowerShell: a syntax error should be reported as such rather
# than as a mysterious scenario failure.
foreach ($script in @($guardScript, $assetGuardScript, $prepublishGuardScript)) {
    $tokens = $null; $errors = $null
    $null = [System.Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
    if ($errors -and $errors.Count -gt 0) {
        throw "A guard extracted from $workflowPath does not parse: $($errors[0].Message)"
    }
}
Write-Host "release workflow: $workflowPath"
Write-Host "scratch root:     $WorkRoot"
Write-Host "test version:     $TEST_VERSION (tag $TEST_TAG)"
Write-Host ""

# --- synthetic world --------------------------------------------------------
# The scenario repositories are built from scratch rather than cloned from the
# checkout on purpose: CI checks out with a shallow history, and a shallow clone
# cannot answer the ancestry question the guard asks. Everything the guards read
# is therefore synthesised here - an `origin` bare repo holding one commit on the
# default branch, containing a `rust/Cargo.toml` that carries the real project
# version so scenario A is gated by the same string the release would be.
$origin = Join-Path $WorkRoot "origin.git"
$seed = Join-Path $WorkRoot "seed"
& git init --quiet --bare $origin
& git -C $origin symbolic-ref HEAD "refs/heads/$DEFAULT_BRANCH"
& git init --quiet --initial-branch=$DEFAULT_BRANCH $seed
New-Item -ItemType Directory -Path (Join-Path $seed "rust\src") -Force | Out-Null
@"
[package]
name = "zhfd-checkin"
version = "$TEST_VERSION"
edition = "2024"

[[bin]]
name = "zhfd-checkin"
path = "src/main.rs"
"@ | Set-Content -LiteralPath (Join-Path $seed "rust\Cargo.toml") -Encoding utf8
Set-Content -LiteralPath (Join-Path $seed "rust\src\main.rs") -Encoding utf8 -Value "fn main() {}"
& git -C $seed @GIT_IDENTITY add -A
& git -C $seed @GIT_IDENTITY commit --quiet -m "synthetic release fixture"
& git -C $seed remote add origin $origin
& git -C $seed push --quiet origin $DEFAULT_BRANCH
& git -C $origin symbolic-ref HEAD "refs/heads/$DEFAULT_BRANCH"
$masterTip = (& git -C $seed rev-parse $DEFAULT_BRANCH | Out-String).Trim()

function New-ScratchRepo {
    param([string]$Name, [string]$Checkout = "")
    $path = Join-Path $WorkRoot $Name
    & git clone --quiet --no-hardlinks $origin $path
    if ($LASTEXITCODE -ne 0) { throw "git clone failed for $Name" }
    & git -C $path checkout --quiet $DEFAULT_BRANCH
    if ($LASTEXITCODE -ne 0) { throw "git checkout $DEFAULT_BRANCH failed for $Name" }
    if ($Checkout) {
        & git -C $path checkout --quiet $Checkout
        if ($LASTEXITCODE -ne 0) { throw "git checkout $Checkout failed for $Name" }
    }
    return $path
}

function Invoke-InRepo {
    param([string]$Tree, [string]$Script, [hashtable]$Env, [string[]]$Argv = @())
    Push-Location $Tree
    try {
        foreach ($key in $Env.Keys) { Set-Item -Path "Env:$key" -Value $Env[$key] }
        $text = (& pwsh -NoProfile -File $Script @Argv 2>&1 | Out-String)
        if ($null -eq $text) { $text = "" }
        return $text
    } finally {
        Pop-Location
        foreach ($key in $Env.Keys) { Remove-Item -Path "Env:$key" -ErrorAction SilentlyContinue }
    }
}

$script:Ok = 0
$script:Bad = 0
function Assert {
    param([string]$Name, [string]$Output, [bool]$ExpectPass, [string]$Marker)
    $passed = ($Output -match $Marker)
    $verdict = if ($passed) { "PASS" } else { "REFUSED" }
    if ($passed -eq $ExpectPass) {
        $script:Ok++
        Write-Host ("ok       {0,-7} {1}" -f $verdict, $Name)
    } else {
        $script:Bad++
        $wanted = if ($ExpectPass) { "PASS" } else { "REFUSED" }
        Write-Host ("MISMATCH want={0} got={1}  {2}" -f $wanted, $verdict, $Name)
    }
    if ($Output -eq "") { return }
    $line = ($Output -split "`n" | Where-Object { $_ -match $Marker } | Select-Object -First 1)
    if ($null -ne $line -and $line.Trim().Length -gt 0) { Write-Host ("         {0}" -f $line.Trim()) }
}

# --- release guard ----------------------------------------------------------
function Invoke-ReleaseGuard {
    param([string]$Tree, [string]$RefType, [string]$RefName, [string]$DefaultBranch = $DEFAULT_BRANCH)
    Invoke-InRepo $Tree $guardScript @{
        GITHUB_REF_TYPE = $RefType
        GITHUB_REF_NAME = $RefName
        GITHUB_REF      = "refs/$($RefType)s/$($RefName)"
        GITHUB_SHA      = (& git -C $Tree rev-parse HEAD | Out-String).Trim()
        DEFAULT_BRANCH  = $DefaultBranch
    }
}

$t = New-ScratchRepo "r1-lightweight" $masterTip
& git -C $t tag $TEST_TAG
Assert "release guard: lightweight $TEST_TAG on the checked-out commit" (Invoke-ReleaseGuard $t "tag" $TEST_TAG) $true "release_guard=PASS"

$t = New-ScratchRepo "r2-annotated" $masterTip
& git -C $t @GIT_IDENTITY tag -a $TEST_TAG -m "annotated tag"
Assert "release guard: annotated $TEST_TAG on the checked-out commit" (Invoke-ReleaseGuard $t "tag" $TEST_TAG) $true "release_guard=PASS"

$t = New-ScratchRepo "r3-branch"
Assert "release guard: workflow_dispatch on a branch ref" (Invoke-ReleaseGuard $t "branch" "master") $false "release_guard=PASS"

$t = New-ScratchRepo "r4-wrong-version" $masterTip
& git -C $t tag $WRONG_TAG
Assert "release guard: tag disagrees with the Cargo version" (Invoke-ReleaseGuard $t "tag" $WRONG_TAG) $false "release_guard=PASS"

$t = New-ScratchRepo "r5-prerelease" $masterTip
& git -C $t tag "$($TEST_TAG)-rc.1"
Assert "release guard: pre-release tag suffix" (Invoke-ReleaseGuard $t "tag" "$($TEST_TAG)-rc.1") $false "release_guard=PASS"

$t = New-ScratchRepo "r6-newer-head" $masterTip
& git -C $t tag $TEST_TAG
& git -C $t @GIT_IDENTITY commit --quiet --allow-empty -m "docs-only follow-up"
Assert "release guard: tag exists but HEAD is a newer commit" (Invoke-ReleaseGuard $t "tag" $TEST_TAG) $false "release_guard=PASS"

$t = New-ScratchRepo "r7-unmerged" $masterTip
& git -C $t checkout --quiet -b feature/sim
& git -C $t @GIT_IDENTITY commit --quiet --allow-empty -m "unmerged work"
& git -C $t tag $TEST_TAG
Assert "release guard: tag on a commit the default branch lacks" (Invoke-ReleaseGuard $t "tag" $TEST_TAG) $false "release_guard=PASS"

$t = New-ScratchRepo "r8-missing-tag" $masterTip
Assert "release guard: tag named but never created" (Invoke-ReleaseGuard $t "tag" $TEST_TAG) $false "release_guard=PASS"

$t = New-ScratchRepo "r9-bad-default" $masterTip
& git -C $t tag $TEST_TAG
Assert "release guard: default branch missing on the remote" (Invoke-ReleaseGuard $t "tag" $TEST_TAG "no-such-branch") $false "release_guard=PASS"

# --- asset guard ------------------------------------------------------------
$crate = New-ScratchRepo "asset-crate" $masterTip
$crateCommit = (& git -C $crate rev-parse --short=12 HEAD | Out-String).Trim()

function Invoke-AssetGuard {
    param([string[]]$ZipNames)
    $dist = Join-Path $crate "dist"
    if (Test-Path -LiteralPath $dist) { Remove-Item -LiteralPath $dist -Recurse -Force }
    if ($null -ne $ZipNames) {
        New-Item -ItemType Directory -Path $dist -Force | Out-Null
        foreach ($name in $ZipNames) {
            Set-Content -LiteralPath (Join-Path $dist $name) -Encoding ascii -Value "placeholder"
        }
    }
    Invoke-InRepo $crate $assetGuardScript @{}
}

$goodName = "zhfd-checkin-windows-x64-$TEST_TAG-$crateCommit.zip"
$wrongVersionName = "zhfd-checkin-windows-x64-$WRONG_TAG-$crateCommit.zip"
Assert "asset guard: single zip, correct name" (Invoke-AssetGuard @($goodName)) $true "asset_guard=PASS"
Assert "asset guard: single zip, wrong version in the name" (Invoke-AssetGuard @($wrongVersionName)) $false "asset_guard=PASS"
Assert "asset guard: single zip, wrong commit in the name" (Invoke-AssetGuard @("zhfd-checkin-windows-x64-$TEST_TAG-000000000000.zip")) $false "asset_guard=PASS"
Assert "asset guard: two zips (stale artifact left behind)" (Invoke-AssetGuard @($goodName, $wrongVersionName)) $false "asset_guard=PASS"
Assert "asset guard: no zip at all" (Invoke-AssetGuard @()) $false "asset_guard=PASS"
Assert "asset guard: dist directory missing" (Invoke-AssetGuard $null) $false "asset_guard=PASS"

# --- pre-publish guard ------------------------------------------------------
# This guard reads the remote tag and asks the GitHub API about an existing
# Release, so its scenarios need (a) an `origin` they control per scenario and
# (b) a stand-in for `gh`. Each scenario therefore gets its own bare origin, and
# a compiled stub answers the API call from a JSON file.
$ghBin = Join-Path $WorkRoot "ghbin"
$stubSource = Join-Path $PSScriptRoot "GhStub.cs"
if (-not (Test-Path -LiteralPath $stubSource)) {
    throw "gh stub source not found: $stubSource"
}
New-Item -ItemType Directory -Path $ghBin -Force | Out-Null
$csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path -LiteralPath $csc)) { throw "csc.exe not found at $csc" }
& $csc /nologo /target:exe "/out:$(Join-Path $ghBin 'gh.exe')" $stubSource | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Failed to compile the gh stub from $stubSource." }

function New-PublishScenario {
    param([string]$Name)
    $root = Join-Path $WorkRoot $Name
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    $scenarioOrigin = Join-Path $root "origin.git"
    $seedRepo = Join-Path $root "seed"
    & git init --quiet --bare $scenarioOrigin
    & git init --quiet --initial-branch=$DEFAULT_BRANCH $seedRepo
    New-Item -ItemType Directory -Path (Join-Path $seedRepo "rust\src") -Force | Out-Null
    @"
[package]
name = "zhfd-checkin"
version = "$TEST_VERSION"
edition = "2024"

[[bin]]
name = "zhfd-checkin"
path = "src/main.rs"
"@ | Set-Content -LiteralPath (Join-Path $seedRepo "rust\Cargo.toml") -Encoding utf8
    Set-Content -LiteralPath (Join-Path $seedRepo "rust\src\main.rs") -Encoding utf8 -Value "fn main() {}"
    & git -C $seedRepo @GIT_IDENTITY add -A
    & git -C $seedRepo @GIT_IDENTITY commit --quiet -m "synthetic release fixture"
    & git -C $seedRepo remote add origin $scenarioOrigin
    & git -C $seedRepo push --quiet origin $DEFAULT_BRANCH
    & git -C $scenarioOrigin symbolic-ref HEAD "refs/heads/$DEFAULT_BRANCH"
    $clone = Join-Path $root "clone"
    & git clone --quiet $scenarioOrigin $clone
    & git -C $clone checkout --quiet $DEFAULT_BRANCH
    if ($LASTEXITCODE -ne 0) { throw "clone/checkout failed for $Name" }
    return [pscustomobject]@{ Root = $root; Clone = $clone }
}

function Set-GhAnswer {
    param([pscustomobject]$Scenario, [int]$ExitCode, [string]$Stdout = "", [string]$Stderr = "")
    $spec = [ordered]@{ exit_code = $ExitCode; stdout = $Stdout; stderr = $Stderr }
    $spec | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $Scenario.Root "gh-spec.json") -Encoding utf8
}

function Invoke-PrepublishGuard {
    param([pscustomobject]$Scenario)
    $savedPath = $env:PATH
    Push-Location $Scenario.Clone
    try {
        $env:PATH = "$ghBin;$savedPath"
        $env:GH_STUB_SPEC = Join-Path $Scenario.Root "gh-spec.json"
        $env:GH_TOKEN = "stub-token"
        $env:REPOSITORY = "owner/repo"
        $env:GITHUB_REF_NAME = $TEST_TAG
        $text = (& pwsh -NoProfile -File $prepublishGuardScript 2>&1 | Out-String)
        if ($null -eq $text) { $text = "" }
        return $text
    } finally {
        Pop-Location
        $env:PATH = $savedPath
        Remove-Item Env:GH_STUB_SPEC, Env:GH_TOKEN, Env:REPOSITORY, Env:GITHUB_REF_NAME -ErrorAction SilentlyContinue
    }
}

function New-ReleaseJson {
    param([string[]]$AssetNames)
    $assets = @($AssetNames | ForEach-Object { [ordered]@{ name = $_ } })
    [ordered]@{ tag_name = $TEST_TAG; assets = $assets } | ConvertTo-Json -Depth 5 -Compress
}

function Get-ScenarioAssetName {
    param([pscustomobject]$Scenario)
    $short = (& git -C $Scenario.Clone rev-parse --short=12 HEAD | Out-String).Trim()
    "zhfd-checkin-windows-x64-$TEST_TAG-$short.zip"
}

function Add-ScenarioTag {
    param([pscustomobject]$Scenario)
    & git -C $Scenario.Clone tag $TEST_TAG
    & git -C $Scenario.Clone push --quiet origin "refs/tags/$TEST_TAG"
}

$s = New-PublishScenario "p1-no-release"
Add-ScenarioTag $s
Set-GhAnswer $s -ExitCode 1 -Stderr "gh: Not Found (HTTP 404)"
Assert "pre-publish: tag unchanged, no existing Release" (Invoke-PrepublishGuard $s) $true "prepublish_guard=PASS"

$s = New-PublishScenario "p2-same-asset"
Add-ScenarioTag $s
Set-GhAnswer $s -ExitCode 0 -Stdout (New-ReleaseJson @((Get-ScenarioAssetName $s)))
Assert "pre-publish: existing Release carries this commit's asset" (Invoke-PrepublishGuard $s) $true "prepublish_guard=PASS"

$s = New-PublishScenario "p3-foreign-asset"
Add-ScenarioTag $s
Set-GhAnswer $s -ExitCode 0 -Stdout (New-ReleaseJson @("zhfd-checkin-windows-x64-$TEST_TAG-000000000000.zip"))
Assert "pre-publish: existing Release carries another commit's asset" (Invoke-PrepublishGuard $s) $false "prepublish_guard=PASS"

$s = New-PublishScenario "p4-tag-moved"
Add-ScenarioTag $s
& git -C $s.Clone @GIT_IDENTITY commit --quiet --allow-empty -m "moved"
& git -C $s.Clone push --quiet --force origin "refs/tags/$TEST_TAG"
Set-GhAnswer $s -ExitCode 1 -Stderr "gh: Not Found (HTTP 404)"
Assert "pre-publish: remote tag moved to a newer commit" (Invoke-PrepublishGuard $s) $false "prepublish_guard=PASS"

$s = New-PublishScenario "p5-tag-gone"
Add-ScenarioTag $s
& git -C $s.Clone push --quiet origin ":refs/tags/$TEST_TAG"
Set-GhAnswer $s -ExitCode 1 -Stderr "gh: Not Found (HTTP 404)"
Assert "pre-publish: remote tag no longer exists" (Invoke-PrepublishGuard $s) $false "prepublish_guard=PASS"

$s = New-PublishScenario "p6-gh-error"
Add-ScenarioTag $s
Set-GhAnswer $s -ExitCode 1 -Stderr "gh: Bad credentials (HTTP 401)"
Assert "pre-publish: cannot tell whether a Release exists" (Invoke-PrepublishGuard $s) $false "prepublish_guard=PASS"

$s = New-PublishScenario "p7-annotated"
& git -C $s.Clone @GIT_IDENTITY tag -a $TEST_TAG -m "annotated tag"
& git -C $s.Clone push --quiet origin "refs/tags/$TEST_TAG"
Set-GhAnswer $s -ExitCode 1 -Stderr "gh: Not Found (HTTP 404)"
Assert "pre-publish: annotated tag resolves to the same commit" (Invoke-PrepublishGuard $s) $true "prepublish_guard=PASS"

$s = New-PublishScenario "p8-empty-release"
Add-ScenarioTag $s
Set-GhAnswer $s -ExitCode 0 -Stdout (New-ReleaseJson @())
Assert "pre-publish: existing Release has no assets yet" (Invoke-PrepublishGuard $s) $true "prepublish_guard=PASS"

# --- the real repository (opt-in: -CheckRealTag) -----------------------------
# The scenarios above are synthetic, so they prove the guard's logic but not that
# it accepts *this* project's real tag. That check has to run the guard against
# the real checkout, and the guard fetches the default branch - which writes
# remote-tracking refs into the real `.git`. That is why it is opt-in: the
# default run stays read-only with respect to this repository.
if (-not $CheckRealTag) {
    Write-Host "skipped  release guard: real-repository check is opt-in (-CheckRealTag)"
} else {
    $realTag = $TEST_TAG
    & git -C $repoRoot rev-parse -q --verify "refs/tags/$realTag" | Out-Null
    $hasRealTag = ($LASTEXITCODE -eq 0)
    $headCommit = (& git -C $repoRoot rev-parse HEAD | Out-String).Trim()
    $tagCommit = if ($hasRealTag) { (& git -C $repoRoot rev-parse "$realTag^{commit}" | Out-String).Trim() } else { "" }
    if (-not ($hasRealTag -and $headCommit -eq $tagCommit)) {
        Write-Host "skipped  release guard: the checkout is not the released tag $realTag (the normal state on a branch or pull request)"
    } else {
        Write-Host "note     -CheckRealTag runs the guard against this repository, including its 'git fetch origin'"
        $out = Invoke-InRepo $repoRoot $guardScript @{
            GITHUB_REF_TYPE = "tag"
            GITHUB_REF_NAME = $realTag
            GITHUB_REF      = "refs/tags/$realTag"
            GITHUB_SHA      = $headCommit
            DEFAULT_BRANCH  = $DEFAULT_BRANCH
        }
        Assert "release guard: the real $realTag at the checked-out commit" $out $true "release_guard=PASS"
    }
}

Write-Host ""
Write-Host "scenarios ok: $script:Ok, mismatched: $script:Bad"
if (-not $KeepScratch -and (Test-Path -LiteralPath $WorkRoot)) {
    Remove-Item -LiteralPath $WorkRoot -Recurse -Force
    Write-Host "scratch removed: $WorkRoot"
} else {
    Write-Host "scratch kept:    $WorkRoot"
}
if ($script:Bad -gt 0) {
    throw "$($script:Bad) release-guard scenario(s) did not behave as expected."
}
Write-Host "RELEASE_GUARD_SIMULATION_PASS"