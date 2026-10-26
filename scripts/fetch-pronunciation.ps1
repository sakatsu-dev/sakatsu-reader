<#
.SYNOPSIS
    Install Tofugu's open Japanese vocabulary pronunciation archive for offline playback.

.DESCRIPTION
    Keeps only MP3 recordings and indexes them by the exact written form plus a normalised
    reading. OGG duplicates and every unrelated archive member are discarded. The previous
    working pack is replaced only after the complete archive and its licence validate.

    Source: https://github.com/tofugu/japanese-vocabulary-pronunciation-audio
    Licence: CC BY-SA 4.0, Tofugu and WaniKani
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\audio\tofugu-vocabulary')
)

$ErrorActionPreference = 'Stop'
$SOURCE = 'https://github.com/tofugu/japanese-vocabulary-pronunciation-audio/archive/refs/heads/master.zip'
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("sakatsu-pronunciation-{0}" -f [guid]::NewGuid())
$archive = Join-Path $work 'tofugu-pronunciation-master.zip'
$pending = "$Out.pending"
$backup = "$Out.old"

function Hiragana([string]$Text) {
    $builder = [System.Text.StringBuilder]::new()
    foreach ($character in $Text.Normalize([System.Text.NormalizationForm]::FormC).ToCharArray()) {
        $code = [int][char]$character
        if ($code -ge 0x30A1 -and $code -le 0x30F6) { $code -= 0x60 }
        [void]$builder.Append([char]$code)
    }
    $builder.ToString()
}

function Is-Mp3([byte[]]$Bytes) {
    if ($Bytes.Length -ge 3 -and $Bytes[0] -eq 0x49 -and $Bytes[1] -eq 0x44 -and $Bytes[2] -eq 0x33) {
        return $true
    }
    $limit = [math]::Min($Bytes.Length - 1, 4096)
    for ($index = 0; $index -lt $limit; $index++) {
        if ($Bytes[$index] -eq 0xff -and (($Bytes[$index + 1] -band 0xe0) -eq 0xe0)) { return $true }
    }
    return $false
}

try {
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    Write-Host 'Tofugu Japanese vocabulary pronunciation audio (CC BY-SA 4.0)' -ForegroundColor Cyan
    Invoke-WebRequest -Uri $SOURCE -OutFile $archive -UseBasicParsing

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    try {
        if ($zip.Entries.Count -gt 30000) { throw 'archive contains too many entries' }
        if (Test-Path $pending) { Remove-Item -LiteralPath $pending -Recurse -Force }
        New-Item -ItemType Directory -Force -Path (Join-Path $pending 'mp3') | Out-Null
        $keys = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
        $index = [System.Collections.Generic.List[string]]::new()
        $licence = $null
        [long]$total = 0

        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            if ($name -match '(^|/)\.\.(/|$)' -or $name.StartsWith('/')) { throw "unsafe archive path: $name" }
            $lower = $name.ToLowerInvariant()
            if ($lower.EndsWith('/license') -or $lower.EndsWith('/license.md')) {
                if ($entry.Length -gt 128KB) { throw 'licence file is too large' }
                $reader = [System.IO.StreamReader]::new($entry.Open(), [System.Text.Encoding]::UTF8)
                try { $licence = $reader.ReadToEnd() } finally { $reader.Dispose() }
                continue
            }
            if (-not ($lower.Contains('/lib/mp3/') -and $lower.EndsWith('.mp3'))) { continue }
            if ($entry.Length -le 0 -or $entry.Length -gt 5MB) { throw "recording has an invalid size: $name" }
            $total += $entry.Length
            if ($total -gt 2GB) { throw 'expanded audio pack is too large' }

            $filename = [System.IO.Path]::GetFileName($name)
            # Greedy on the surface so this matches Rust's rsplit-once rule if a spelling ever
            # contains the decorative opening bracket itself.
            # Both lenticular brackets - the archive uses the black pair; see `split_reading`
            # in `core/src/setup/enrichment.rs` for why the white one is still accepted.
            if ($filename -notmatch '^(.*)[【〖]([^【】〖〗]*)[】〗]\.mp3$') { throw "recording filename has no reading: $filename" }
            $surface = $matches[1].Trim().Normalize([System.Text.NormalizationForm]::FormC)
            $reading = Hiragana $matches[2].Trim()
            if (-not $surface -or -not $reading -or
                $surface.IndexOfAny([char[]]@("`t", "`r", "`n")) -ge 0 -or
                $reading.IndexOfAny([char[]]@("`t", "`r", "`n")) -ge 0) {
                throw "recording filename has an unsafe key: $filename"
            }
            $key = "$surface`t$reading"
            if (-not $keys.Add($key)) { throw "duplicate pronunciation key: $surface / $reading" }

            $memory = [System.IO.MemoryStream]::new()
            $stream = $entry.Open()
            try { $stream.CopyTo($memory) } finally { $stream.Dispose() }
            $bytes = $memory.ToArray(); $memory.Dispose()
            if (-not (Is-Mp3 $bytes)) { throw "recording is not MP3 audio: $filename" }
            $output = '{0:D6}.mp3' -f $index.Count
            [System.IO.File]::WriteAllBytes((Join-Path $pending "mp3\$output"), $bytes)
            $index.Add("$key`t$output")
        }
        if ($index.Count -eq 0) { throw 'archive contains no MP3 recordings' }
        if (-not $licence) { throw 'archive contains no licence' }
        $index.Sort([System.StringComparer]::Ordinal)
        [System.IO.File]::WriteAllLines((Join-Path $pending 'index.tsv'), $index, [System.Text.UTF8Encoding]::new($false))
        [System.IO.File]::WriteAllText((Join-Path $pending 'LICENSE'), $licence, [System.Text.UTF8Encoding]::new($false))
        [System.IO.File]::WriteAllText(
            (Join-Path $pending 'meta.json'),
            '{"title":"Tofugu Japanese Vocabulary Pronunciation Audio","licence":"CC BY-SA 4.0","source":"https://github.com/tofugu/japanese-vocabulary-pronunciation-audio"}',
            [System.Text.UTF8Encoding]::new($false))
    }
    finally {
        $zip.Dispose()
    }

    if (Test-Path $backup) { Remove-Item -LiteralPath $backup -Recurse -Force }
    if (Test-Path $Out) { Move-Item -LiteralPath $Out -Destination $backup }
    try {
        Move-Item -LiteralPath $pending -Destination $Out
    }
    catch {
        if (Test-Path $backup) { Move-Item -LiteralPath $backup -Destination $Out }
        throw
    }
    if (Test-Path $backup) { Remove-Item -LiteralPath $backup -Recurse -Force }
    Write-Host "Installed $($index.Count) offline recordings." -ForegroundColor Green
}
finally {
    if (Test-Path $pending) { Remove-Item -LiteralPath $pending -Recurse -Force }
    if (Test-Path $work) { Remove-Item -LiteralPath $work -Recurse -Force }
}
