# WPG Local Testing & Benchmark Automation Script
param(
    [int]$Quality = 80,
    [int]$Speed = 6
)

$ErrorActionPreference = "Stop"

# Resolve path to wpg.exe
$wpgExe = Join-Path $PSScriptRoot "..\target\release\wpg.exe"
if (-not (Test-Path $wpgExe)) {
    $wpgExe = "wpg"
    if (-not (Get-Command $wpgExe -ErrorAction SilentlyContinue)) {
        Write-Error "wpg.exe not found! Please run 'cargo build --release' first."
        exit 1
    }
}

$inputDir = Join-Path $PSScriptRoot "input"
$outputDir = Join-Path $PSScriptRoot "output"

if (-not (Test-Path $inputDir)) { New-Item -ItemType Directory -Path $inputDir | Out-Null }
if (-not (Test-Path $outputDir)) { New-Item -ItemType Directory -Path $outputDir | Out-Null }

# Check for images in input folder
function Get-TestImages {
    Get-ChildItem -Path $inputDir -File | Where-Object { $_.Extension -match '^\.(png|jpe?g|webp)$' }
}

$images = Get-TestImages

if ($images.Count -eq 0) {
    Write-Host "[*] No images found in input/. Copying sample image to test with..." -ForegroundColor Yellow
    $sampleSrc = Join-Path $PSScriptRoot "..\sample_test.png"
    if (Test-Path $sampleSrc) {
        Copy-Item $sampleSrc (Join-Path $inputDir "sample.png")
        $images = Get-TestImages
    }
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  WPG Local Testing Suite (Quality: $Quality, Speed: $Speed)  " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$results = @()

foreach ($img in $images) {
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($img.Name)
    $wpgOut = Join-Path $outputDir "$baseName.wpg"
    $pngOut = Join-Path $outputDir "$baseName-decoded.png"
    $prevOut = Join-Path $outputDir "$baseName-preview.png"

    Write-Host "`n>> Encoding: $($img.Name)..." -ForegroundColor Green
    & $wpgExe encode $img.FullName $wpgOut --quality $Quality --speed $Speed

    Write-Host ">> Decoding full image to PNG..." -ForegroundColor DarkGray
    & $wpgExe decode $wpgOut $pngOut

    Write-Host ">> Extracting 16-byte micro-preview..." -ForegroundColor DarkGray
    & $wpgExe decode $wpgOut $prevOut --preview-only

    $origSize = (Get-Item $img.FullName).Length
    $wpgSize = (Get-Item $wpgOut).Length
    $ratio = [math]::Round(($wpgSize / $origSize) * 100, 1)
    $savings = [math]::Round(100 - $ratio, 1)

    $savingsStr = if ($savings -ge 0) { "-$savings%" } else { "+$([math]::Abs($savings))%" }

    $results += [PSCustomObject]@{
        "Image"            = $img.Name
        "Original (Bytes)" = "{0:N0}" -f $origSize
        "WPG (Bytes)"      = "{0:N0}" -f $wpgSize
        "Ratio"            = "$ratio%"
        "Savings"          = $savingsStr
    }
}

Write-Host "`nCompression Results:" -ForegroundColor Cyan
$results | Format-Table -AutoSize

if ($images.Count -gt 0) {
    $firstImg = $images[0].FullName
    Write-Host "`nRunning Bitstream-Level Comparison Benchmark on: $($images[0].Name)" -ForegroundColor Cyan
    & $wpgExe bench $firstImg --quality $Quality
}

Write-Host "`n[+] Done! Check your encoded files in: $outputDir" -ForegroundColor Green
Write-Host "[+] Open 'playground/index.html' in your browser to view them!" -ForegroundColor Yellow
