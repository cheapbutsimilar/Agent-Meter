param(
    [Parameter(Mandatory=$true)][string]$AgyPath,
    [Parameter(Mandatory=$true)][string]$PywinptySource,
    [string]$AgentMeterHome = "$env:LOCALAPPDATA\AgentMeter",
    [string]$AgentMeterData = "$env:APPDATA\dev.agentmeter.app"
)
$ErrorActionPreference = 'Stop'
$homeDir = Join-Path $AgentMeterHome 'antigravity-bridge'
$node = (Get-Command node.exe).Source
$python = (& python -c 'import sys; print(sys.executable)').Trim()
$pythonw = Join-Path (Split-Path $python) 'pythonw.exe'
$AgyPath = (Resolve-Path $AgyPath).Path
$meterSettings = Join-Path $AgentMeterData 'settings.json'
$agySettings = "$env:USERPROFILE\.gemini\antigravity-cli\settings.json"
$exe = Join-Path $AgentMeterHome 'agentmeter.exe'
foreach ($file in @($pythonw, $meterSettings, $agySettings, $exe, "$PywinptySource\winpty\__init__.py")) {
    if (!(Test-Path $file)) { throw "Required file missing: $file" }
}
# agy 1.5.16's Windows statusLine runner mishandles quoted paths. Reject unsupported
# shell paths rather than attempting an unsafe or ambiguous quoting workaround.
if ($node -match '[\s&|<>^%!"''()]' -or $homeDir -match '[\s&|<>^%!"''()]') {
    throw 'This collector version requires shell-safe Node and installation paths without spaces.'
}
$meter = Get-Content -Raw -Encoding UTF8 $meterSettings | ConvertFrom-Json
$agy = Get-Content -Raw -Encoding UTF8 $agySettings | ConvertFrom-Json
$onboard = Get-Content -Raw -Encoding UTF8 "$env:USERPROFILE\.gemini\antigravity-cli\cache\onboarding.json" | ConvertFrom-Json
if ($onboard.onboardingComplete -ne $true) {
    throw 'Complete the official CLI sign-in/onboarding interactively first.'
}
$flags = @{}
foreach ($key in @('consumerOnboardingComplete','enterpriseOnboardingComplete','onboardingComplete')) {
    if ($onboard.$key -is [bool]) { $flags[$key] = $onboard.$key }
}
$bridge = Join-Path $homeDir 'antigravity-bridge.mjs'
$query = "$node $bridge query-background"
$oldQuery = "$node $bridge query"
$oldCapture = "$node $bridge capture"
if ($meter.providers.gemini.command -notin @('', $oldQuery, $query)) {
    throw 'An unrelated Gemini user command is configured; refusing to replace it.'
}

New-Item -ItemType Directory -Path $homeDir -Force | Out-Null
# Restrict this dedicated folder and inherited child files to this Windows user,
# SYSTEM and local administrators. No administrative elevation is required.
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls.exe $homeDir /inheritance:r /grant:r "*${sid}:(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not restrict collector directory permissions.' }
$backup = Join-Path $homeDir ("backup-background-" + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $backup | Out-Null
Copy-Item $meterSettings "$backup\agentmeter-settings.json"
Copy-Item $agySettings "$backup\antigravity-settings.json"
$vendor = Join-Path $homeDir 'vendor'
New-Item -ItemType Directory -Path $vendor -Force | Out-Null
Copy-Item "$PywinptySource\winpty" $vendor -Recurse -Force
Get-ChildItem $PywinptySource -Directory -Filter 'pywinpty-*.dist-info' | Copy-Item -Destination $vendor -Recurse -Force
Copy-Item "$PSScriptRoot\antigravity-bridge.mjs" $bridge -Force
Copy-Item "$PSScriptRoot\antigravity-collector.py" $homeDir -Force
$utf8 = New-Object Text.UTF8Encoding($false)
function Write-JsonFile($file, $value) {
    [IO.File]::WriteAllText("$file.background.tmp", ($value | ConvertTo-Json -Depth 100), $utf8)
    Move-Item "$file.background.tmp" $file -Force
}
Write-JsonFile "$homeDir\collector.json" @{
    pythonw = $pythonw
    worker = "$homeDir\antigravity-collector.py"
    node = $node
    agy = $AgyPath
    agySha256 = (Get-FileHash $AgyPath -Algorithm SHA256).Hash.ToLowerInvariant()
    intervalSeconds = 180
    onboarding = $flags
}
$running = @(Get-Process agentmeter -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe })
$running | Stop-Process
$running | Wait-Process -ErrorAction SilentlyContinue
try {
    # The interactive CLI no longer needs a custom status line. Do not replace
    # any unrelated user customization.
    if ($agy.statusLine.command -eq $oldCapture) {
        $agy.PSObject.Properties.Remove('statusLine')
        Write-JsonFile $agySettings $agy
    }
    $meter.providers.gemini.command = $query
    $meter.providers.gemini.useApi = $false
    $meter.providers.gemini.enabled = $true
    $meter.providers.gemini.visible = $true
    $meter.providers.gemini.budget5h = 0
    $meter.providers.gemini.budgetWeek = 0
    Write-JsonFile $meterSettings $meter
} catch {
    Copy-Item "$backup\agentmeter-settings.json" $meterSettings -Force
    Copy-Item "$backup\antigravity-settings.json" $agySettings -Force
    throw
} finally {
    if ($running.Count -gt 0) { Start-Process $exe }
}
Write-Output "Background collector installed: $homeDir"
Write-Output "Backup: $backup"
Write-Output 'No startup task/service was added. AgentMeter starts a short-lived hidden collector when due.'
