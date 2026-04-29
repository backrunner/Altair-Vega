param(
    [string]$Url = $env:ALTAIR_VEGA_BIN_URL,
    [string]$RuntimeParent,
    [switch]$KeepRuntime,
    [switch]$Help,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CommandArgs
)

$ErrorActionPreference = 'Stop'

function Show-Usage {
    @"
Usage: scripts/startup.ps1 [-Url <binary-url>] [-RuntimeParent <dir>] [-KeepRuntime] [-Help] [-- <args...>]

Downloads a native Altair Vega executable into a disposable runtime workspace,
runs it with any remaining arguments, and removes the downloaded executable and
runtime state on exit unless -KeepRuntime is set.

Environment:
  ALTAIR_VEGA_BIN_URL       Explicit binary URL when -Url is omitted.
  ALTAIR_VEGA_GITHUB_REPO   GitHub repo for latest release lookup.
  ALTAIR_VEGA_RUNTIME_ROOT, TMPDIR, TMP, and TEMP are set for the launched process.
"@
}

function Get-DefaultAltairVegaBinaryUrl {
    $repo = if ($env:ALTAIR_VEGA_GITHUB_REPO) { $env:ALTAIR_VEGA_GITHUB_REPO } else { 'EL-File4138/Altair-Vega' }
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        'X64' { $machine = 'x86_64' }
        'Arm64' { $machine = 'aarch64' }
        default { throw "unsupported architecture for default binary URL: $arch; pass -Url or set ALTAIR_VEGA_BIN_URL" }
    }

    "https://github.com/$repo/releases/latest/download/altair-vega-windows-$machine.exe"
}

function Download-AltairVegaBinary {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SourceUrl,
        [Parameter(Mandatory = $true)]
        [string]$TargetPath
    )

    $uri = [Uri]$SourceUrl
    if ($uri.IsFile) {
        Copy-Item -LiteralPath $uri.LocalPath -Destination $TargetPath -Force
        return
    }

    Invoke-WebRequest -Uri $SourceUrl -OutFile $TargetPath
}

if ($Help) {
    Show-Usage
    exit 0
}

if (-not $Url) {
    $Url = Get-DefaultAltairVegaBinaryUrl
}

if (-not $RuntimeParent) {
    $RuntimeParent = [System.IO.Path]::GetTempPath()
}

$workspace = Join-Path $RuntimeParent ("altair-vega." + [Guid]::NewGuid().ToString('N'))
$runtimeRoot = Join-Path $workspace 'runtime'
$tmpRoot = Join-Path $runtimeRoot 'tmp'
$binaryPath = Join-Path $workspace 'altair-vega.exe'

New-Item -ItemType Directory -Path $workspace -Force | Out-Null
New-Item -ItemType Directory -Path $runtimeRoot -Force | Out-Null
New-Item -ItemType Directory -Path $tmpRoot -Force | Out-Null

$previousEnv = @{
    ALTAIR_VEGA_RUNTIME_ROOT = $env:ALTAIR_VEGA_RUNTIME_ROOT
    ALTAIR_VEGA_KEEP_RUNTIME = $env:ALTAIR_VEGA_KEEP_RUNTIME
    TMPDIR = $env:TMPDIR
    TMP = $env:TMP
    TEMP = $env:TEMP
}

try {
    Download-AltairVegaBinary -SourceUrl $Url -TargetPath $binaryPath

    $env:ALTAIR_VEGA_RUNTIME_ROOT = $runtimeRoot
    $env:ALTAIR_VEGA_KEEP_RUNTIME = if ($KeepRuntime) { '1' } else { '0' }
    $env:TMPDIR = $tmpRoot
    $env:TMP = $tmpRoot
    $env:TEMP = $tmpRoot

    if ($CommandArgs.Count -gt 0 -and $CommandArgs[0] -eq '--') {
        if ($CommandArgs.Count -gt 1) {
            $CommandArgs = $CommandArgs[1..($CommandArgs.Count - 1)]
        } else {
            $CommandArgs = @()
        }
    }

    & $binaryPath @CommandArgs
    $exitCode = if ($null -ne $LASTEXITCODE) { $LASTEXITCODE } else { 0 }
}
finally {
    foreach ($entry in $previousEnv.GetEnumerator()) {
        if ($null -eq $entry.Value) {
            Remove-Item -Path ("Env:" + $entry.Key) -ErrorAction SilentlyContinue
        }
        else {
            Set-Item -Path ("Env:" + $entry.Key) -Value $entry.Value
        }
    }

    if ($KeepRuntime) {
        Write-Host "keeping Altair Vega runtime at $workspace"
    }
    else {
        Remove-Item -LiteralPath $workspace -Recurse -Force -ErrorAction SilentlyContinue
    }
}

exit $exitCode
