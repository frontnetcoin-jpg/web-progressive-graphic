$dist = "dist_release"
if (Test-Path $dist) {
    Remove-Item -Recurse -Force $dist
}
New-Item -ItemType Directory -Path $dist | Out-Null

$binaries = @(
    "wpg.exe",
    "wpg-enc.exe",
    "wpg-dec.exe",
    "wpg-lab.exe",
    "wpg-view.exe",
    "wpg_thumb.dll"
)

foreach ($b in $binaries) {
    $p = "target\release\$b"
    if (Test-Path $p) {
        Copy-Item $p $dist
        Write-Host "Included: $b"
    }
}

$assets = @(
    "wpg-decoder.wasm",
    "wpg-element.js",
    "wpg-worker.js",
    "README.md",
    "DOCUMENTATION.md",
    "SPECIFICATION.md",
    "LICENSE"
)

foreach ($a in $assets) {
    if (Test-Path $a) {
        Copy-Item $a $dist
        Write-Host "Included: $a"
    }
}

$zipNames = @("wpg-windows-x86_64.zip", "wpg-v1.0.3-windows-x86_64.zip")
foreach ($zip in $zipNames) {
    if (Test-Path $zip) {
        Remove-Item -Force $zip
    }
    Compress-Archive -Path "$dist\*" -DestinationPath $zip
    Write-Host "Created release archive: $zip"
    Get-Item $zip | Select-Object Name, Length
}

