# WPG Local Testing Playground

This folder is the local test bench for **WPG (Web Progressive Graphic)**.

Drop in photographs, logos, or screenshots, see how many bytes you save, and view the results in the browser with `<wpg-img>`.

---

## Playground layout

```
playground/
├── input/             # Put source images here (.jpg, .png, .webp)
├── output/            # Encoded .wpg files and decoded .png files
├── test-encode.ps1    # One-click encode and benchmark script
├── index.html         # Standalone page for the offline <wpg-img> bundle
└── README.md          # This guide
```

---

## Three steps

### Step 1: Add images
Copy any image (for example `photo.jpg` or `logo.png`) into **`playground/input/`**.

### Step 2: Run the one-click test
Open PowerShell in the `playground` folder and run:
```powershell
.\test-encode.ps1
```
The script will:
1. Encode every image in `input/` to `.wpg` under `output/`.
2. Report the **KB and percent saved** against the source file.
3. Extract the **16-byte micro-preview**.
4. Run a **benchmark** (PSNR, SSIM, bits per pixel).

---

### Step 3: View the result in a browser
Open `index.html` in either of these ways:
* Open `playground/index.html` directly (or with Live Server). That page loads `wpg-bundle.js`, an older main-thread bundle with Wasm embedded as base64, so it also works from `file://`.
* Or start the showcase server:
  ```powershell
  # From the wpg repository root:
  .\target\release\wpg.exe serve --port 3000
  ```
  Then open **[http://localhost:3000](http://localhost:3000)**. That page runs the production `<wpg-img>` (`/wpg-element.js` as a module, plus `/wpg-worker.js`), with a live preview of `/api/sample.wpg` and a **Reload / Test Transition** button.

---

## Useful CLI commands

```powershell
# 1. Encode one image at a chosen quality (1..100):
..\target\release\wpg.exe encode input\photo.jpg output\photo.wpg --quality 80

# 2. Decode the .wpg file back to .png:
..\target\release\wpg.exe decode output\photo.wpg output\restored.png

# 3. Extract only the 16-byte micro-preview:
..\target\release\wpg.exe decode output\photo.wpg output\preview.png --preview-only

# 4. Print container info and flags:
..\target\release\wpg.exe info output\photo.wpg

# 5. Full comparison (WPG vs WebP vs AVIF vs PNG vs JPEG):
..\target\release\wpg.exe bench input\photo.jpg --quality 80
```
