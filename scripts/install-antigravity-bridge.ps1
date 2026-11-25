param(
    [string]$AgentMeterHome = "$env:LOCALAPPDATA\AgentMeter",
    [string]$AgentMeterData = "$env:APPDATA\dev.agentmeter.app",
    [string]$AntigravityHome = "$env:USERPROFILE\.gemini\antigravity-cli"
)
$ErrorActionPreference = 'Stop'
$node = (Get-Command node.exe -ErrorAction Stop).Source
$bridgeHome = Join-Path $AgentMeterHome 'antigravity-bridge'
$bridge = Join-Path $bridgeHome 'antigravity-bridge.mjs'
$meterSettings = Join-Path $AgentMeterData 'settings.json'
$agySettings = Join-Path $AntigravityHome 'settings.json'
$exe = Join-Path $AgentMeterHome 'agentmeter.exe'

if (!(Test-Path $meterSettings) -or !(Test-Path $agySettings) -or !(Test-Path $exe)) {
    throw 'Install and start AgentMeter and Antigravity CLI once before installing the bridge.'
}
# agy 1.5.16 on Windows passes embedded double quotes literally to Node.
# Encoded PowerShell handles paths with spaces without embedded shell quoting.
function New-BridgeCommand([string]$Mode) {
    if ($node -notmatch '\s' -and $bridge -notmatch '\s') {
        return "$node $bridge $Mode"
    }
    $command = "& '" + $node.Replace("'", "''") + "' '" + $bridge.Replace("'", "''") + "' $Mode"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    return "powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded"
}
$captureCommand = New-BridgeCommand 'capture'
$queryCommand = New-BridgeCommand 'query'
$agy = Get-Content -Raw -Encoding UTF8 $agySettings | ConvertFrom-Json
$meter = Get-Content -Raw -Encoding UTF8 $meterSettings | ConvertFrom-Json
if ($agy.statusLine.command -and $agy.statusLine.command -ne $captureCommand) {
    throw 'An existing custom Antigravity statusLine is configured. Preserve/chain it manually first.'
}
if ($meter.providers.gemini.command -and $meter.providers.gemini.command -ne $queryCommand) {
    throw 'An existing Gemini user command is configured. Preserve it before replacing it.'
}

New-Item -ItemType Directory -Path $bridgeHome -Force | Out-Null
$backup = Join-Path $bridgeHome ("backup-" + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
New-Item -ItemType Directory -Path $backup | Out-Null
Copy-Item $meterSettings (Join-Path $backup 'agentmeter-settings.json')
Copy-Item $agySettings (Join-Path $backup 'antigravity-settings.json')
Copy-Item (Join-Path $PSScriptRoot 'antigravity-bridge.mjs') $bridge -Force

$running = @(Get-Process agentmeter -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe })
$running | Stop-Process
$running | Wait-Process -ErrorAction SilentlyContinue
$cachePath = Join-Path $AgentMeterData 'usage-cache.json'
if (Test-Path $cachePath) {
    Copy-Item $cachePath (Join-Path $backup 'usage-cache.json')
}
$utf8 = New-Object Text.UTF8Encoding($false)
function Write-JsonFile($File, $Value) {
    $temp = "$File.bridge.tmp"
    [IO.File]::WriteAllText($temp, ($Value | ConvertTo-Json -Depth 100), $utf8)
    Move-Item $temp $File -Force
}
try {
    $agy | Add-Member -Force -NotePropertyName statusLine -NotePropertyValue ([pscustomobject]@{
        type = 'command'
        command = $captureCommand
        enabled = $true
        stack_with_default = $true
    })
    Write-JsonFile $agySettings $agy
    $meter.providers.gemini.command = $queryCommand
    $meter.providers.gemini.useApi = $false
    $meter.providers.gemini.enabled = $true
    $meter.providers.gemini.visible = $true
    $meter.providers.gemini.budget5h = 0
    $meter.providers.gemini.budgetWeek = 0
    $meter.providers.gemini.hiddenWindows = @()
    Write-JsonFile $meterSettings $meter
    # Keep all usage history and all other providers. Remove only obsolete Gemini
    # quota state, otherwise empty bridge windows could fall back to old CLI limits.
    if (Test-Path $cachePath) {
        $cache = Get-Content -Raw -Encoding UTF8 $cachePath | ConvertFrom-Json
        if ($cache.providerState) {
            $cache.providerState.PSObject.Properties.Remove('gemini')
            Write-JsonFile $cachePath $cache
        }
    }
} catch {
    Copy-Item (Join-Path $backup 'agentmeter-settings.json') $meterSettings -Force
    Copy-Item (Join-Path $backup 'antigravity-settings.json') $agySettings -Force
    if (Test-Path (Join-Path $backup 'usage-cache.json')) {
        Copy-Item (Join-Path $backup 'usage-cache.json') $cachePath -Force
    }
    throw
} finally {
    if ($running.Count -gt 0) { Start-Process $exe }
}
Write-Output "Bridge installed: $bridge"
Write-Output "Settings backup: $backup"
Write-Output 'Start/restart Antigravity CLI and run /usage. AgentMeter reads the bridge every poll interval.'
