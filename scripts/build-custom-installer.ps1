$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$srcTauri = Join-Path $repo "src-tauri"
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path -LiteralPath $cargo)) {
  $cargo = "cargo"
}

Push-Location $srcTauri
try {
  & $cargo build --release --locked --bin omnafk
  $payload = Join-Path $srcTauri "target\release\omnafk.exe"
  if (-not (Test-Path -LiteralPath $payload)) {
    throw "Missing payload executable at $payload"
  }

  $oldPayload = $env:OMNAFK_PAYLOAD_EXE
  $env:OMNAFK_PAYLOAD_EXE = $payload
  & $cargo build --release --locked --bin omnafk-setup
}
finally {
  $env:OMNAFK_PAYLOAD_EXE = $oldPayload
  Pop-Location
}

$dist = Join-Path $repo "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item -LiteralPath (Join-Path $srcTauri "target\release\omnafk-setup.exe") -Destination (Join-Path $dist "OMNAFK-Setup.exe") -Force
Get-Item -LiteralPath (Join-Path $dist "OMNAFK-Setup.exe")
