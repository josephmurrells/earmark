# Packages a release build into dist/: a zip with earmark.exe, the README,
# Earmark's licences, every third-party licence, and SHA256SUMS.txt.
# Run after `cargo build --release --locked`; the release workflow does both.
param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'

$FontLicenses = @('OFL.txt', 'UFL.txt', 'Hack-Regular.txt', 'emoji-icon-font-mit-license.txt', 'egui-icons.txt')

function Invoke-Native {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Assert-StaticRuntime {
    param([string]$Path)
    $text = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($Path))
    if ($text -match '(?i)vcruntime140|msvcp140|concrt140|api-ms-win-crt-') {
        throw "$Path links the Visual C++ runtime dynamically; build with +crt-static"
    }
}

function Assert-Zip {
    param([string]$Path, [string[]]$Entries)
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $names = $archive.Entries | ForEach-Object { $_.FullName }
        foreach ($entry in $Entries) {
            if ($names -notcontains $entry) {
                throw "$Path is missing $entry"
            }
        }
    }
    finally {
        $archive.Dispose()
    }
}

# Finds the licences of the fonts egui builds into the exe.
function Get-FontDirectory {
    $encoding = [Console]::OutputEncoding
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    try {
        $metadata = & cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
    }
    finally {
        [Console]::OutputEncoding = $encoding
    }
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $found = [regex]::Matches(($metadata -join ''), '"manifest_path":"([^"]*epaint_default_fonts-[^"]*?Cargo\.toml)"')
    if ($found.Count -ne 1) {
        throw "expected one epaint_default_fonts package, found $($found.Count)"
    }
    $manifest = [regex]::Unescape($found[0].Groups[1].Value)
    Join-Path (Split-Path -Parent $manifest) 'fonts'
}

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $dist = Join-Path $root 'dist'
    $name = "Earmark-$Version-windows-x64"
    $stage = Join-Path $dist $name
    $licenses = Join-Path $stage 'licenses'
    $zip = Join-Path $dist "$name.zip"

    if (Test-Path $dist) {
        Remove-Item -Recurse -Force $dist
    }
    New-Item -ItemType Directory -Path $licenses | Out-Null

    $built = Join-Path $root 'target\release\earmark.exe'
    Assert-StaticRuntime $built
    Copy-Item $built $stage
    foreach ($file in @('README.md', 'LICENSE-MIT', 'LICENSE-APACHE')) {
        Copy-Item (Join-Path $root $file) $stage
    }

    $fonts = Get-FontDirectory
    foreach ($file in $FontLicenses) {
        Copy-Item (Join-Path $fonts $file) $licenses
    }

    Invoke-Native cargo @('about', 'generate', '--locked', '-c', 'release/about.toml', 'release/about.hbs', '-o', (Join-Path $stage 'THIRD-PARTY-LICENSES.txt'))

    Invoke-Native "$env:SystemRoot\System32\tar.exe" @('-a', '-c', '-f', $zip, '-C', $dist, $name)
    Assert-Zip $zip @("$name/earmark.exe", "$name/README.md", "$name/LICENSE-MIT", "$name/LICENSE-APACHE", "$name/THIRD-PARTY-LICENSES.txt", "$name/licenses/OFL.txt", "$name/licenses/UFL.txt")

    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS.txt'), "$hash  $(Split-Path -Leaf $zip)`n")

    Get-ChildItem $dist -File | Format-Table Name, Length
}
finally {
    Pop-Location
}
