param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

$ffmpegAsset = 'ffmpeg-master-latest-win64-lgpl.zip'
$ffmpegBase = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest'
$ytDlpVersion = '2026.10.26'
$ytDlpAsset = 'yt-dlp.exe'
$ytDlpBase = "https://github.com/yt-dlp/yt-dlp/releases/download/$ytDlpVersion"
$denoVersion = '2.9.8'
$denoAsset = 'deno-x86_64-pc-windows-msvc.zip'
$denoBase = "https://github.com/denoland/deno/releases/download/v$denoVersion"
$curlVersion = '8.23.0_9'
$curlAsset = "curl-$curlVersion-win64-mingw.zip"
$curlUrl = "https://curl.se/windows/dl-$curlVersion/$curlAsset"
$curlSha256 = '89cb71930f4242d884e8e349463455932ab1d089b20f4f99318261a3cbff9167'

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("mymedia-runtime-" + [guid]::NewGuid())
$bin = Join-Path $OutputDirectory 'bin'
$licenses = Join-Path $OutputDirectory 'licenses'
New-Item -ItemType Directory -Force -Path $work, $bin, $licenses | Out-Null

function Confirm-ManifestChecksum {
    param([string]$Manifest, [string]$Asset, [string]$File)
    $escaped = [regex]::Escape($Asset)
    $line = Get-Content $Manifest | Where-Object { $_ -match "^[0-9a-fA-F]{64}\s+\*?$escaped$" } | Select-Object -First 1
    if (-not $line) { throw "Checksum not found for $Asset" }
    $expected = ($line -split '\s+')[0].ToUpperInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $File).Hash.ToUpperInvariant()
    if ($expected -ne $actual) { throw "Checksum mismatch for $Asset" }
}

try {
    $ffmpegArchive = Join-Path $work $ffmpegAsset
    $ffmpegChecksums = Join-Path $work 'ffmpeg-checksums.sha256'
    Invoke-WebRequest -Uri "$ffmpegBase/$ffmpegAsset" -OutFile $ffmpegArchive
    Invoke-WebRequest -Uri "$ffmpegBase/checksums.sha256" -OutFile $ffmpegChecksums
    Confirm-ManifestChecksum $ffmpegChecksums $ffmpegAsset $ffmpegArchive
    $ffmpegExtract = Join-Path $work 'ffmpeg'
    Expand-Archive -Path $ffmpegArchive -DestinationPath $ffmpegExtract
    Copy-Item (Get-ChildItem $ffmpegExtract -Recurse -File -Filter 'ffmpeg.exe' | Select-Object -First 1).FullName (Join-Path $bin 'ffmpeg.exe')
    Copy-Item (Get-ChildItem $ffmpegExtract -Recurse -File -Filter 'ffprobe.exe' | Select-Object -First 1).FullName (Join-Path $bin 'ffprobe.exe')

    $ytDlpFile = Join-Path $work $ytDlpAsset
    $ytDlpChecksums = Join-Path $work 'yt-dlp-checksums.sha256'
    Invoke-WebRequest -Uri "$ytDlpBase/$ytDlpAsset" -OutFile $ytDlpFile
    Invoke-WebRequest -Uri "$ytDlpBase/SHA2-256SUMS" -OutFile $ytDlpChecksums
    Confirm-ManifestChecksum $ytDlpChecksums $ytDlpAsset $ytDlpFile
    Copy-Item $ytDlpFile (Join-Path $bin 'yt-dlp.exe')

    $denoArchive = Join-Path $work $denoAsset
    $denoChecksum = Join-Path $work 'deno-checksum.sha256sum'
    Invoke-WebRequest -Uri "$denoBase/$denoAsset" -OutFile $denoArchive
    Invoke-WebRequest -Uri "$denoBase/$denoAsset.sha256sum" -OutFile $denoChecksum
    $denoChecksumMatch = Get-Content $denoChecksum | Select-String -Pattern 'Hash\s+:\s+([0-9A-Fa-f]{64})' | Select-Object -First 1
    if (-not $denoChecksumMatch) { throw "Checksum not found for $denoAsset" }
    $expectedDenoHash = $denoChecksumMatch.Matches[0].Groups[1].Value.ToUpperInvariant()
    $actualDenoHash = (Get-FileHash -Algorithm SHA256 $denoArchive).Hash.ToUpperInvariant()
    if ($expectedDenoHash -ne $actualDenoHash) { throw "Checksum mismatch for $denoAsset" }
    $denoExtract = Join-Path $work 'deno'
    Expand-Archive -Path $denoArchive -DestinationPath $denoExtract
    Copy-Item (Join-Path $denoExtract 'deno.exe') (Join-Path $bin 'deno.exe')

    $curlArchive = Join-Path $work $curlAsset
    Invoke-WebRequest -Uri $curlUrl -OutFile $curlArchive
    $actualCurlHash = (Get-FileHash -Algorithm SHA256 $curlArchive).Hash.ToLowerInvariant()
    if ($actualCurlHash -ne $curlSha256) { throw "Checksum mismatch for $curlAsset" }
    $curlExtract = Join-Path $work 'curl'
    Expand-Archive -Path $curlArchive -DestinationPath $curlExtract
    $curlExecutable = Get-ChildItem $curlExtract -Recurse -File -Filter 'curl.exe' | Select-Object -First 1
    if (-not $curlExecutable) { throw 'curl.exe not found in downloaded archive' }
    Copy-Item $curlExecutable.FullName (Join-Path $bin 'curl.exe')
    Get-ChildItem $curlExecutable.DirectoryName -File -Filter '*.dll' | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $bin $_.Name)
    }

    $ffmpegLicenses = Join-Path $licenses 'ffmpeg'
    $curlLicenses = Join-Path $licenses 'curl'
    New-Item -ItemType Directory -Force -Path $ffmpegLicenses, $curlLicenses | Out-Null
    Get-ChildItem $ffmpegExtract -Recurse -File | Where-Object {
        $_.Name -match '^(COPYING|LICENSE)'
    } | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $ffmpegLicenses $_.Name) -Force
    }
    Get-ChildItem $curlExtract -Recurse -File | Where-Object {
        $_.Name -match '^(COPYING|LICENSE)'
    } | ForEach-Object {
        $component = $_.Directory.Name
        Copy-Item $_.FullName (Join-Path $curlLicenses "$component-$($_.Name)") -Force
    }
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/yt-dlp/yt-dlp/$ytDlpVersion/LICENSE" -OutFile (Join-Path $licenses 'yt-dlp-UNLICENSE.txt')
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/denoland/deno/v$denoVersion/LICENSE.md" -OutFile (Join-Path $licenses 'deno-MIT.txt')

    $ffmpegVersion = ((& (Join-Path $bin 'ffmpeg.exe') -version | Select-Object -First 1) -replace '^ffmpeg version ', '').Trim()
    $ytDlpActual = (& (Join-Path $bin 'yt-dlp.exe') --version).Trim()
    $denoActual = ((& (Join-Path $bin 'deno.exe') --version | Select-Object -First 1) -replace '^deno ', '').Trim()
    $curlActual = ((& (Join-Path $bin 'curl.exe') --version | Select-Object -First 1) -split ' ')[1]
    @{
        ffmpeg = $ffmpegVersion
        'yt-dlp' = $ytDlpActual
        deno = $denoActual
        curl = $curlActual
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'versions.json')
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
