$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $metadata = cargo metadata --no-deps --offline --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $package = $metadata.packages | Where-Object { $_.name -eq 'fyle' } | Select-Object -First 1
    if ($null -eq $package) { throw 'fyle package not found' }

    cargo build --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'release build failed' }

    $compiler = Get-Command iscc.exe -ErrorAction Stop
    & $compiler.Source "/DAppVersion=$($package.version)" (Join-Path $PSScriptRoot 'fyle.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed' }
}
finally {
    Pop-Location
}
