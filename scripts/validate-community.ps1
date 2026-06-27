$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$manifestPath = Join-Path $root "community\manifest.json"
$profilesPath = Join-Path $root "community\profiles"

if (-not (Test-Path -LiteralPath $manifestPath)) {
  throw "Missing community manifest: $manifestPath"
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$errors = New-Object System.Collections.Generic.List[string]

function Add-ValidationError([string]$Message) {
  $errors.Add($Message) | Out-Null
}

function Test-DateString([string]$Value) {
  $parsed = [datetime]::MinValue
  return [datetime]::TryParseExact(
    $Value,
    "yyyy-MM-dd",
    [Globalization.CultureInfo]::InvariantCulture,
    [Globalization.DateTimeStyles]::None,
    [ref]$parsed
  )
}

$allowedActions = @(
  "Space tap",
  "W tap",
  "Camera nudge",
  "Mouse wiggle",
  "Scroll tick",
  "Right click"
)
$allowedFallbacks = @("FocusFlick", "CameraNudge", "Normal")
$allowedStatuses = @("stable", "watch", "degraded")

if (-not ($manifest.version -is [ValueType]) -or [double]$manifest.version -ne [math]::Floor([double]$manifest.version) -or $manifest.version -lt 1) {
  Add-ValidationError "manifest.version must be a positive integer."
}

if ([string]::IsNullOrWhiteSpace($manifest.updated) -or -not (Test-DateString $manifest.updated)) {
  Add-ValidationError "manifest.updated must use YYYY-MM-DD."
}

if ($null -eq $manifest.games) {
  Add-ValidationError "manifest.games is required."
}

if ($null -eq $manifest.detection) {
  Add-ValidationError "manifest.detection is required."
}

$gameNames = New-Object System.Collections.Generic.HashSet[string]
if ($null -ne $manifest.games) {
  foreach ($game in $manifest.games.PSObject.Properties) {
    $exe = $game.Name
    $entry = $game.Value

    if ($exe -cne $exe.ToLowerInvariant()) {
      Add-ValidationError "$exe must be lowercase."
    }
    if (-not $exe.EndsWith(".exe")) {
      Add-ValidationError "$exe must end with .exe."
    }
    $gameNames.Add($exe) | Out-Null

    if ($allowedActions -notcontains $entry.action) {
      Add-ValidationError "$exe action must be one of: $($allowedActions -join ', ')."
    }
    if ($entry.interval -lt 10 -or $entry.interval -gt 3600) {
      Add-ValidationError "$exe interval must be between 10 and 3600 seconds."
    }
    if ($entry.confidence -lt 0 -or $entry.confidence -gt 1) {
      Add-ValidationError "$exe confidence must be between 0 and 1."
    }
    foreach ($field in @("detection_confidence", "action_confidence", "monitor_confidence")) {
      if ($null -ne $entry.$field -and ($entry.$field -lt 0 -or $entry.$field -gt 1)) {
        Add-ValidationError "$exe $field must be between 0 and 1."
      }
    }
    if ($entry.reports -lt 0) {
      Add-ValidationError "$exe reports must be zero or greater."
    }
    if ($allowedStatuses -notcontains $entry.status) {
      Add-ValidationError "$exe status must be one of: $($allowedStatuses -join ', ')."
    }
    if ($null -ne $entry.verified -and -not (Test-DateString $entry.verified)) {
      Add-ValidationError "$exe verified must use YYYY-MM-DD."
    }

    if ($null -ne $entry.fallback_order) {
      foreach ($fallback in $entry.fallback_order) {
        if ($allowedFallbacks -notcontains $fallback) {
          Add-ValidationError "$exe fallback '$fallback' is not supported."
        }
      }
    }
  }
}

if (Test-Path -LiteralPath $profilesPath) {
  foreach ($profilePath in Get-ChildItem -LiteralPath $profilesPath -Filter "*.json") {
    $profile = Get-Content -LiteralPath $profilePath.FullName -Raw | ConvertFrom-Json
    if ([string]::IsNullOrWhiteSpace($profile.exe)) {
      Add-ValidationError "$($profilePath.Name) must include exe."
      continue
    }

    $profileExe = $profile.exe.ToLowerInvariant()
    if (-not $gameNames.Contains($profileExe)) {
      Add-ValidationError "$($profilePath.Name) points to '$profileExe', which is not in manifest.games."
    }
    if ($allowedActions -notcontains $profile.recommended_action) {
      Add-ValidationError "$($profilePath.Name) recommended_action is not supported."
    }
    if ($profile.recommended_interval -lt 10 -or $profile.recommended_interval -gt 3600) {
      Add-ValidationError "$($profilePath.Name) recommended_interval must be between 10 and 3600 seconds."
    }
    if ($profile.confidence -lt 0 -or $profile.confidence -gt 1) {
      Add-ValidationError "$($profilePath.Name) confidence must be between 0 and 1."
    }
    if ($allowedStatuses -notcontains $profile.status) {
      Add-ValidationError "$($profilePath.Name) status must be one of: $($allowedStatuses -join ', ')."
    }
  }
}

if ($errors.Count -gt 0) {
  $errors | ForEach-Object { Write-Error $_ }
  exit 1
}

Write-Host "Community manifest and profiles are valid."
