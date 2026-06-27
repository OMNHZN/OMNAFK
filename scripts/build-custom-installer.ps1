param(
  [switch]$KeepBuildCache
)

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

  # Bundle the ViGEmBus driver installer when present, so the setup can offer
  # to install it for the Gamepad nudge action. Honor a caller-provided
  # OMNAFK_VIGEM_EXE, else fall back to the vendored copy.
  $vigem = $env:OMNAFK_VIGEM_EXE
  if (-not $vigem) {
    $vendored = Join-Path $repo "vendor\ViGEmBus_Setup_x64.exe"
    if (Test-Path -LiteralPath $vendored) {
      $vigem = $vendored
    }
  }

  $oldPayload = $env:OMNAFK_PAYLOAD_EXE
  $oldVigem = $env:OMNAFK_VIGEM_EXE
  $env:OMNAFK_PAYLOAD_EXE = $payload
  if ($vigem) {
    $env:OMNAFK_VIGEM_EXE = $vigem
    Write-Host "Bundling ViGEmBus driver from $vigem"
  }
  & $cargo build --release --locked --bin omnafk-setup
}
finally {
  $env:OMNAFK_PAYLOAD_EXE = $oldPayload
  $env:OMNAFK_VIGEM_EXE = $oldVigem
  Pop-Location
}

$dist = Join-Path $repo "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item -LiteralPath (Join-Path $srcTauri "target\release\omnafk-setup.exe") -Destination (Join-Path $dist "OMNAFK-Setup.exe") -Force

if (-not $KeepBuildCache) {
  Push-Location $srcTauri
  try {
    & $cargo clean
  }
  finally {
    Pop-Location
  }
}

Get-Item -LiteralPath (Join-Path $dist "OMNAFK-Setup.exe")
