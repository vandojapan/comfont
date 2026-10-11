$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$releaseDir = Join-Path $projectRoot 'release'
[IO.Directory]::CreateDirectory($releaseDir) | Out-Null
$variant = 'beta'
$flavor = 'release'
$stageDir = Join-Path $projectRoot ('target/package-' + $variant)
[IO.Directory]::CreateDirectory($stageDir) | Out-Null
$ini = '[package]' + [Environment]::NewLine + 'id=vandojapan.comfont' + [Environment]::NewLine + 'name=comfont' + [Environment]::NewLine + 'information=comfont v0.2.0 β' + [Environment]::NewLine + 'uninstallSubFolderFile=0' + [Environment]::NewLine
[IO.File]::WriteAllText((Join-Path $stageDir 'package.ini'), $ini, [Text.UTF8Encoding]::new($false))
$zipPath = Join-Path $releaseDir ('vandojapan.comfont-v0.2.0-' + $variant + '.au2pkg.zip')
$stream = [IO.File]::Open($zipPath, [IO.FileMode]::Create)
$zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
try {
  $entries = @(
    @((Join-Path $projectRoot ('target/' + $flavor + '/compositefont_editor.dll')), 'Plugin/comfont.aux2'),
    @((Join-Path $projectRoot 'scripts/合成フォント.anm2'), 'Script/合成フォント.anm2'),
    @((Join-Path $projectRoot 'resource/material-icons/LICENSE.txt'), 'Plugin/comfont-licenses/material-icons.txt'),
    @((Join-Path $projectRoot 'resource/lucide/LICENSE.txt'), 'Plugin/comfont-licenses/lucide.txt'),
    @((Join-Path $projectRoot 'resource/unicode/LICENSE.txt'), 'Plugin/comfont-licenses/unicode.txt'),
    @((Join-Path $stageDir 'package.ini'), 'package.ini')
  )
  foreach ($pair in $entries) { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $pair[0], $pair[1], [IO.Compression.CompressionLevel]::Optimal) | Out-Null }
} finally { $zip.Dispose(); $stream.Dispose() }
$check = [IO.Compression.ZipFile]::OpenRead($zipPath)
try {
  if ($check.Entries.Count -ne 6) { throw 'Unexpected package entry count' }
  foreach ($entry in $check.Entries) { Write-Output ($variant + ': ' + $entry.FullName + ' (' + $entry.Length + ' bytes)') }
  $plugin = $check.GetEntry('Plugin/comfont.aux2')
  $reader = $plugin.Open()
  try { $sha = [Security.Cryptography.SHA256]::Create(); $packagedHash = [BitConverter]::ToString($sha.ComputeHash($reader)) } finally { $reader.Dispose(); $sha.Dispose() }
  $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $projectRoot ('target/' + $flavor + '/compositefont_editor.dll'))).Hash
  if ($packagedHash.Replace('-', '') -ne $sourceHash) { throw 'Packaged plugin hash differs from build output' }
} finally { $check.Dispose() }
