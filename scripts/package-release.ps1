$dist = "dist_release"
if (Test-Path $dist) {
    Remove-Item -Recurse -Force $dist
}
New-Item -ItemType Directory -Path $dist | Out-Null

Copy-Item "target\release\wpg.exe" $dist
Copy-Item "target\release\wpg-enc.exe" $dist
Copy-Item "target\release\wpg-dec.exe" $dist
Copy-Item "wpg-decoder.wasm" $dist
Copy-Item "wpg-element.js" $dist
Copy-Item "README.md" $dist
Copy-Item "DOCUMENTATION.md" $dist
Copy-Item "SPECIFICATION.md" $dist

$zip = "wpg-v1.0.0-windows-x86_64.zip"
if (Test-Path $zip) {
    Remove-Item -Force $zip
}

Compress-Archive -Path "$dist\*" -DestinationPath $zip
Write-Host "Created release archive: $zip"
Get-Item $zip | Select-Object Name, Length
