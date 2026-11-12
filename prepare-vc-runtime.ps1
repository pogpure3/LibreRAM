# SPDX-License-Identifier: GPL-2.0-only
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1')
$package = Join-Path $PSScriptRoot 'src-tauri\resources\imdisk-setup\vc_redist.x64.exe'
$url = 'https://download.visualstudio.microsoft.com/download/pr/78d061c5-9ef3-42ab-98ed-3309d1095dc4/9C02C9ED120F671B900E6B9471D890E3B2414117A7E53F4AE2E855466FA2B7C9/VC_redist.x64.exe'
$sha256 = '9C02C9ED120F671B900E6B9471D890E3B2414117A7E53F4AE2E855466FA2B7C9'

function Assert-MicrosoftRuntime([string]$Path) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $stream = [IO.File]::OpenRead($Path)
        try { $actual = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '') }
        finally { $stream.Dispose() }
    } finally { $algorithm.Dispose() }
    if ($actual -ne $sha256) {
        throw "Visual C++ Redistributable SHA-256 mismatch: $Path"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '^CN=Microsoft Corporation,') {
        throw "Visual C++ Redistributable has no valid Microsoft signature: $Path"
    }
}

if (Test-Path -LiteralPath $package -PathType Leaf) {
    Assert-MicrosoftRuntime $package
    return
}

$temporary = Join-Path ([IO.Path]::GetTempPath()) ("libreram-vc-redist-" + [Guid]::NewGuid().ToString('N') + '.exe')
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 120 -OutFile $temporary
    Assert-MicrosoftRuntime $temporary
    [IO.File]::Move($temporary, $package)
} finally {
    if (Test-Path -LiteralPath $temporary -PathType Leaf) {
        Remove-Item -LiteralPath $temporary
    }
}
