# Build the Windows installer (`flowers_<version>_x64_en-US.msi`).
#
#   pwsh packaging/windows/build-msi.ps1              # build + package
#   pwsh packaging/windows/build-msi.ps1 -SkipBuild   # package an existing exe
#
# Requirements: Rust (msvc toolchain) and the free WiX v5 CLI:
#
#   dotnet tool install --global wix --version 5.0.2
#   wix extension add -g WixToolset.UI.wixext/5.0.2
#
# WiX v6/v7 refuse to run without accepting their Open Source Maintenance Fee
# licence, which is why this pins v5.

[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$cargoToml = Join-Path $repo 'Cargo.toml'
$wxs = Join-Path $PSScriptRoot 'flowers.wxs'

$version = (Select-String -Path $cargoToml -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw "Could not read the version from $cargoToml" }

Push-Location $repo
try {
    if (-not $SkipBuild) {
        Write-Host "Building flowers-gui $version (release)..." -ForegroundColor Cyan
        cargo build --release --bin flowers-gui
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    }

    $exe = Join-Path $repo 'target\release\flowers-gui.exe'
    if (-not (Test-Path $exe)) { throw "Missing $exe - run without -SkipBuild first." }

    $out = Join-Path $repo "target\release\flowers_${version}_x64_en-US.msi"
    Write-Host "Packaging $out..." -ForegroundColor Cyan

    # WiX's UI shows the licence agreement page; without this it falls back to
    # placeholder "lorem ipsum" text. Convert LICENSE to RTF for that dialog
    # (the result is committed, so a bare `wix build` works too).
    $licenseRtf = Join-Path $PSScriptRoot 'license.rtf'
    $license = Get-Content (Join-Path $repo 'LICENSE') -Raw
    $license = $license.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}')
    $license = ($license -split "`r?`n") -join "`r`n\par`r`n"
    $rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs18 $license}"
    Set-Content -Path $licenseRtf -Value $rtf -Encoding ASCII

    wix build -arch x64 `
        -d "Version=$version" `
        -d "LicenseRtf=$licenseRtf" `
        -ext WixToolset.UI.wixext `
        -o $out `
        $wxs
    if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }

    Write-Host "Wrote $out" -ForegroundColor Green
    Get-Item $out | Select-Object FullName, Length
}
finally {
    Pop-Location
}
