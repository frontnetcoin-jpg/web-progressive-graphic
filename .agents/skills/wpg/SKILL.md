---
name: wpg
description: >-
  Comprehensive engineering, workflow, and testing skill for WPG (Web Progressive Graphic),
  the next-generation AV1-based image format in pure Rust. Use when building, testing,
  benchmarking, encoding/decoding, running batch conversions, managing the Glassmorphic
  Web Showcase GUI or REST API, tuning film grain synthesis, working with the 16-byte zero-CLS
  micro-preview, or integrating the <wpg-img> web component.
---

# WPG (Web Progressive Graphic) Next-Gen Image Format

This skill is the engineering reference for the **WPG (.wpg)** image format: architecture, CLI workflows, the `<wpg-img>` client, and the rules for changing the codebase safely.

---

## 1. Overview and Architecture

WPG is a **pure Rust** image format based on **AV1 Intra**. It drops the ISOBMFF container tax of `.avif` and is built for the modern web:

```
[Input image (PNG, JPEG, WebP, raw RGBA)]
                     │
                     ▼
  ┌──────────────────────────────────────────────┐
  │         Feature Extraction & Pre-pass        │
  │  • 16-Byte Micro-Preview (K-Means 4x4 Grid)  │
  │  • Film Grain Analysis & Laplacian Estimator │
  │  • Denoising Filter (Sensor noise removal)   │
  └──────────────────────┬───────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────┐
  │             Tri-Mode Decision Engine         │
  │  • Mode 0: AV1 Enhanced Intra (rav1e)        │
  │  • Mode 1: Palette Lossless (2D RLE + Zstd)  │
  │  • Mode 2: Modular YCoCg-R Reversible + Zstd │
  └──────────────────────┬───────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────┐
  │          16-Byte Ultra-Lean Container        │
  │  • Magic: b"WPG\x01"                         │
  │  • Dimensions (24-bit W x H)                 │
  │  • Mode, Subsampling, Bitflags, Quality      │
  │  • [Optional 16B] Micro-Preview Block        │
  │  • [Optional 8B] Film Grain Parameter Block  │
  │  • [Optional] Alpha Block (u32 BE length)    │
  │  • Primary Payload (OBU Bitstream / Zstd)    │
  └──────────────────────┬───────────────────────┘
                         │
                         ▼
                  [Final *.wpg file]
```

### Core modules (`src/`)

* [`src/header.rs`](file:///c:/Users/user/Desktop/wpg/src/header.rs): 16-byte binary header, bitflags (`FLAG_ALPHA`, `FLAG_MICRO_PREVIEW`, `FLAG_FILM_GRAIN`, `FLAG_SECONDARY_ENTROPY`, `FLAG_LOSSLESS`), serialization, and validation.
* [`src/preview.rs`](file:///c:/Users/user/Desktop/wpg/src/preview.rs): 16-byte micro-preview (4 RGB K-means colors plus 16 2-bit spatial indices on a 4×4 grid).
* [`src/grain.rs`](file:///c:/Users/user/Desktop/wpg/src/grain.rs): 2D local-extremum camera noise detector (ignores smooth gradients and edges), pre-encode smoothing, and deterministic Xorshift32 grain synthesis on decode. The on-disk block is 8 bytes: `grain_scale`, `grain_seed` (u16 BE), `grain_cutoff`, `chroma_scale`, `blend_mode`, and 2 reserved bytes.
* [`src/color.rs`](file:///c:/Users/user/Desktop/wpg/src/color.rs): sRGB to YUV (4:2:0, 4:2:2, 4:4:4) using BT.709 and BT.601 with unbiased rounding and bilinear chroma upsampling, plus reversible YCoCg-R.
* [`src/lossless.rs`](file:///c:/Users/user/Desktop/wpg/src/lossless.rs): Mode 1 (palette of at most 256 colors, 2D RLE, Zstd) and Mode 2 (YCoCg-R, Paeth/Median predictors, Zstd).
* [`src/av1_codec.rs`](file:///c:/Users/user/Desktop/wpg/src/av1_codec.rs): Multi-threaded pure Rust AV1 intra encoder (`rav1e 0.8` with parallel tiles and still-image speed settings) and decoder (`rav1d`), the calibrated quantizer curve, and `decode_avif_to_rgba` for bitstream-level AVIF decode.
* [`src/encoder.rs`](file:///c:/Users/user/Desktop/wpg/src/encoder.rs) and [`src/decoder.rs`](file:///c:/Users/user/Desktop/wpg/src/decoder.rs): High-level encode and decode pipelines (single cached palette pass). The alpha block is a big-endian `u32` length followed by a Zstd plane.
* [`src/metrics.rs`](file:///c:/Users/user/Desktop/wpg/src/metrics.rs): Pixel-accurate PSNR, SSIM, bits per pixel, and savings.
* [`src/server.rs`](file:///c:/Users/user/Desktop/wpg/src/server.rs): Embedded HTTP server with the **3-Format Head-to-Head Benchmark Studio (`.wpg` vs `.webp` vs `.avif`)**, drag-and-drop `.jpg` dropzone, split view, zoom lens, REST API, and the web component assets.
* [`src/bin/wpg.rs`](file:///c:/Users/user/Desktop/wpg/src/bin/wpg.rs): Unified CLI (`encode`, `decode`, `batch`, `bench`, `info`, `serve`, `view`, `register`).
* [`src/bin/wpg_lab.rs`](file:///c:/Users/user/Desktop/wpg/src/bin/wpg_lab.rs): Automated laboratory rate-distortion sweep engine (`wpg-lab all / run`) and playground server (`wpg-lab serve`).
* [`laboratory/`](file:///c:/Users/user/Desktop/wpg/laboratory/): Curated datasets (`datasets/`), structured JSON sweep runs (`runs/`), and interactive SVG Rate-Distortion playground dashboard (`playground/index.html`).
* [`wpg-element.js`](file:///c:/Users/user/Desktop/wpg/wpg-element.js): ESM `<wpg-img>` client. ReadableStream micro-preview, worker pool, 0-KB Wasm pure-JS Mode 0 fast path, Declarative Shadow DOM / `renderWpgImg()`, and fallbacks.
* [`wpg-worker.js`](file:///c:/Users/user/Desktop/wpg/wpg-worker.js): Off-thread 0-KB Wasm pure-JS ISOBMFF wrapper (`wrapWpgMode0ToAvifJs`) and `applyFilmGrainJs` for Mode 0, plus lazy on-demand Wasm loading for palette/modular RGBA decode and alpha merge, returning a transferable `ImageBitmap`.
* [`crates/wpg-wasm`](file:///c:/Users/user/Desktop/wpg/crates/wpg-wasm/src/lib.rs): On-demand client decoder. `wpg_alloc` / `wpg_free` use one exact allocation size. `wpg_wrap_to_avif` returns `-5` for Mode 1 and Mode 2. `wpg_decode_rgba` decodes those modes. `wpg_apply_alpha` merges the Zstd alpha plane.

---

## 2. CLI Guide (`wpg`)

### Web showcase and viewer
```powershell
.\target\release\wpg.exe serve --port 3000
```
- URL: `http://127.0.0.1:3000`
- Features the **3-Format Head-to-Head Benchmark Studio (`.wpg` vs `.webp` vs `.avif`)** with drag-and-drop `.jpg` upload, podium cards, 1:1 split comparison slider, zoom magnifier, EXIF orientation correction, live quality/speed controls, and 1-click download buttons for `.wpg`, `.webp`, `.avif`, `.png`, and `.bin`.
- The integration card includes a live `<wpg-img>` that automatically updates to preview `/api/latest.wpg` and a **Reload / Test Transition** button.

### Batch conversion
```powershell
# Convert every image in a folder at quality 75:
.\target\release\wpg.exe batch C:\path\to\images -q 75

# Recurse into subdirectories and write a new folder:
.\target\release\wpg.exe batch .\input_photos -o .\output_wpg -q 80 -s 6 -r
```

### Single commands
```powershell
# Encode
.\target\release\wpg.exe encode input.png output.wpg --quality 80 --speed 6

# Decode to PNG
.\target\release\wpg.exe decode output.wpg restored.png

# Extract only the 16-byte micro-preview as a 4x4 PNG
.\target\release\wpg.exe decode output.wpg placeholder.png --preview-only

# Header and container info
.\target\release\wpg.exe info output.wpg

# 3-Format Head-to-Head Benchmark (.wpg vs .webp vs .avif) with PSNR, SSIM, bpp, and optional output folder
.\target\release\wpg.exe bench input.jpg --quality 80 --speed 6 --output-dir .\bench_out

# Native desktop viewer (Win32 GDI with pan and zoom)
.\target\release\wpg.exe view output.wpg
# or directly:
.\target\release\wpg-view.exe output.wpg

# Register with Windows Explorer (no administrator rights)
# Enables double-click, the context menu, and live Explorer thumbnails (wpg_thumb.dll)
.\target\release\wpg.exe register

# Remove the Explorer registration:
.\target\release\wpg.exe unregister

# Laboratory rate-distortion sweeps & interactive playground
cargo run --release --bin wpg-lab -- all
cargo run --release --bin wpg-lab -- run laboratory\datasets\camera_photo.jpg
cargo run --release --bin wpg-lab -- serve --port 8080
```

### Windows shell integration (`wpg_thumb.dll`)
* **Explorer thumbnails:** `crates/wpg-thumb` implements `IThumbnailProvider` and `IInitializeWithFile` in pure Rust. It decodes the AV1 bitstream and returns a 32-bit ARGB DIB at the size Explorer requests.
* **Desktop viewer (`wpg-view.exe`):** Double-buffered GDI canvas, mouse-wheel zoom, drag-to-pan, directory browsing with the arrow keys, PNG export with `S`, fullscreen with `F` / `F11`, the HUD with `I`, and drag-and-drop.
* **Context menu:** Right-click a `.wpg` file for "Open with WPG Viewer", "Convert to PNG", and "WPG Image Analysis".
* **Zero-admin registration:** COM CLSID and ProgID keys are written under `HKCU:\Software\Classes`. No UAC prompt.

---

## 3. Web Component `<wpg-img>`

`wpg-element.js` is an ES module and must be loaded with `type="module"`. The showcase server embeds it with `include_str!` and serves `/wpg-element.js`, `/wpg-worker.js`, and `/wpg-decoder.wasm`.

```html
<script type="module" src="http://localhost:3000/wpg-element.js"></script>

<wpg-img
    src="http://localhost:3000/api/sample.wpg"
    alt="High-Efficiency WPG Asset"
    width="800"
    height="600"
    style="width: 100%; max-width: 800px; border-radius: 12px;">
</wpg-img>
```

The showcase page renders a live `<wpg-img src="/api/sample.wpg" alt="Live WPG Component Preview">` beside the integration snippet. **Reload / Test Transition** sets `src` to `/api/sample.wpg?t=<timestamp>`. The router strips the query string, so the sample route still matches.

### Progressive load
1. **ReadableStream.** `response.body.getReader()` accumulates chunks until `totalLength >= 32`. The first 32 bytes are stitched even when the first packet is only the 16-byte header. Do not assume the first `read()` is at least 32 bytes. This path does not use HTTP Range requests.
2. **Micro-preview.** The 16-byte block becomes an SVG data-URI `<img id="ssr-preview">` with `blur(14px)`. The `<canvas>` stays 4×4 until the final frame. `width` and `height` set `aspect-ratio` so layout shift stays at zero.
3. **Worker & Zero-Wasm Fast Path.** Standard Mode 0 images are wrapped into a 242-byte ISOBMFF header (`wrapWpgMode0ToAvifJs`) and grain-synthesized (`applyFilmGrainJs`) in pure JavaScript inside `wpg-worker.js` with **0 KB of WebAssembly downloaded**. Only when an image uses Mode 1 (Palette), Mode 2 (Modular), `FLAG_ALPHA`, or `FLAG_SECONDARY_ENTROPY` does the worker lazily load `wpg-decoder.wasm` on demand (`wpg_wrap_to_avif` returns `-5` for Mode 1/2, selecting `wpg_decode_rgba`; `FLAG_ALPHA` is merged via `wpg_apply_alpha`). The frame returns with `postMessage(..., [bitmap])`. The worker receives a copy of the buffer.
4. **Fallbacks.** Worker, then Wasm on the main thread, then `POST` to `fallback-url` (default `/api/decode`). `fallback-url="none"` skips the server. A worker crash rejects the pending promise.
5. **`loading="lazy"`.** An `IntersectionObserver` (`rootMargin: 200px`) stays connected. Outside that margin the decode is cancelled, the `ImageBitmap` is closed, and the canvas is cleared. The SVG remains. Coming back fetches the URL again.
6. **SSR.** `renderWpgImg()` is safe to call from Node. It emits Declarative Shadow DOM with the aspect-ratio box and the SVG preview before JavaScript runs.

```js
import { renderWpgImg } from 'wpg-img';

const html = renderWpgImg({
  src: '/images/hero.wpg',
  alt: 'Hero',
  width: 800,
  height: 600,
  bytes: fileOr32BytePrefix,
  loading: 'lazy'
});
```

Other attributes: `wasm-url`, `worker-url`, `fallback-url`.

A Mode 0 payload with `FLAG_SECONDARY_ENTROPY` is Zstd-inflated inside `wpg_wrap_to_avif` and then wrapped. If that inflate fails, the call returns `-5` and the page uses the server fallback.

---

## 4. Public Distribution (npm and GitHub)

* **GitHub repository:** [https://github.com/frontnetcoin-jpg/web-progressive-graphic](https://github.com/frontnetcoin-jpg/web-progressive-graphic) (public, branch `main`)
* **npm package:** [https://www.npmjs.com/package/wpg-img](https://www.npmjs.com/package/wpg-img) (`wpg-img@1.0.3`)
  - Install: `npm install wpg-img`
  - Contents: TypeScript types (`dist/index.d.ts`), ESM client (`dist/index.js`), worker (`dist/wpg-worker.js`), and the on-demand Wasm decoder (`wpg-decoder.wasm`, ~130 KB with LTO). Exports: `<wpg-img>`, `renderWpgImg`, `decodeWpg`, `getWpgWasm`, `getWpgWorkerPool`.

### Installers and release packaging
- **Windows PowerShell:** `scripts/install.ps1`
- **Linux and macOS:** `scripts/install.sh`
- **Release zip:** `scripts/package-release.ps1` builds `wpg-v1.0.0-windows-x86_64.zip`

---

## 5. Local Playground (`playground/`)

- `playground/input/`: Drop `.jpg`, `.png`, or `.webp` files here.
- `playground/output/`: Encoded `.wpg` files and decoded PNGs.
- `playground/test-encode.ps1`: One-click encode, savings report, and benchmarks.
- `playground/index.html`: Drag-and-drop page for the offline bundle.
- `playground/wpg-bundle.js`: Older self-contained bundle with Wasm embedded as base64 so `file:///` works. It has no ReadableStream client, no worker, and no `renderWpgImg()`. The production client is `wpg-element.js`.

---

## 6. SEO and Core Web Vitals

- **CLS 0.000:** The 16-byte header and 16-byte micro-preview reserve the image box in under 0.05 ms.
- **LCP:** 20% to 35% smaller than AVIF on camera photographs, and up to 68% smaller than JPEG.
- **Production result (any2prompt.com):** Mobile PageSpeed moved from **74–75** to **96/100**.
  - Gallery (10 images): **8.69 MB** to **993 KB** (**-88.6%**).
  - Hero visual: **4.19 MB** to **87.9 KB** (**-97.9%**).
- **Integration rules:**
  1. **Lazy loading:** `loading="lazy"` keeps an `IntersectionObserver` with a `200px` root margin. The fetch waits until the image nears the viewport, and the canvas plus `ImageBitmap` are released after it leaves.
  2. **ESM:** Load the production script as `<script type="module" src="/wpg-element.js"></script>`. `defer` on a classic script does not load this file, because the file has `export`.
  3. **No duplicate fetch:** `loadWpg` ignores a repeated `src` while `_loadedUrl` matches. A new query string (`?t=`) starts a new load.
  4. **No-JS crawler fallback:**
     ```html
     <wpg-img src="/images/hero.wpg" alt="Hero Title" loading="lazy">
       <noscript>
         <img src="/images/hero.jpg" alt="Hero Title" width="800" height="600" loading="lazy">
       </noscript>
     </wpg-img>
     ```

---

## 7. Development and Safety Rules

1. **Windows binary lock (`os error 32`):**
   - Before `cargo build --release`, stop any running server (`taskkill /F /IM wpg.exe`). Windows locks the executable while the process is open.
2. **Embedded showcase assets:**
   - `src/server.rs` embeds `wpg-element.js`, `wpg-worker.js`, and `wpg-decoder.wasm` with `include_str!` / `include_bytes!`. Changes to those files or to the showcase HTML are invisible in `wpg.exe` until the next `cargo build --release`.
   - If `cargo` is not on `PATH`, the toolchain binary is `%USERPROFILE%\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe`. Run `target\release\wpg.exe`.
3. **Benchmark measurements:**
   - Never substitute placeholder numbers for third-party formats.
   - AVIF is decoded at bitstream level, skipping the monochrome alpha payload inside the `mdat` box.
   - WebP covers lossy WebP (Google `libwebp-sys`) and lossless WebP (VP8L), with real pixel PSNR and SSIM.
4. **Wasm compatibility:**
   - `crates/wpg-wasm` stays free of POSIX/libc and of `rav1d`, so it compiles for `wasm32-unknown-unknown`. Standard Mode 0 images decode with 0 KB of Wasm via the pure-JS fast path in `wpg-worker.js`; the on-demand Wasm binary (~130 KB with LTO) includes pure-Rust Zstd (`ruzstd`) for alpha, Mode 1, and Mode 2.
   - Build with `cargo build -p wpg-wasm --target wasm32-unknown-unknown --release`, then copy `wpg_wasm.wasm` to `wpg-decoder.wasm`, `packages/wpg-img/wpg-decoder.wasm`, and `packages/wpg-img/dist/wpg-decoder.wasm`.
5. **Master memory:**
   - Read [`MEMORY.md`](file:///c:/Users/user/Desktop/wpg/MEMORY.md) for the format equations and decision history.
6. **WordPerfect disambiguation:**
   - WPG (Web Progressive Graphic) is a 2026 web image format. It is unrelated to the 1980s *WordPerfect Graphics* format (`.wpg`). As with TypeScript (`.ts`) and MPEG Transport Stream (`.ts`), the extension is distinguished by context, the MIME type `image/wpg`, and the magic bytes `b"WPG\x01"`.
7. **Windows Explorer thumbnail provider:**
   - **Stream EOF:** `ISequentialStream::Read` returns `S_FALSE (1)` on the last chunk. Always append bytes when `read_bytes > 0` before stopping, or the AV1 bitstream is truncated (`Error parsing OBU data`).
   - **SystemFileAssociations override:** On Windows 10/11, `PerceivedType = image` makes Explorer fall through to `PhotoMetadataHandler.dll`. Register an explicit override at `HKCU:\Software\Classes\SystemFileAssociations\.wpg\ShellEx\{e357fccd-a995-4576-b01f-234630154e96}`.
   - **Photo frame and icon:** Set `Treatment = 2` for the drop-shadow photo border and `DefaultIcon = %SystemRoot%\System32\imageres.dll,-70`.
   - **Cache flush:** After rebuilding `wpg_thumb.dll`, restart Explorer (`Stop-Process -Name explorer -Force; Start-Process explorer.exe`) so the in-memory thumbnail cache is cleared.
8. **Compression Engine & RDO Calibration:**
   - **Chroma phase alignment:** `sample_chroma_bilinear` in `src/color.rs` must use half-pixel centering with clamping (`((x + 0.5) * 0.5 - 0.5).clamp(0.0, max)`). Any offset causes phase inversion at image borders, degrading chroma PSNR by 1–3 dB.
   - **AV1 Quantizer scale:** In AV1 / `rav1e`, lower quantizer means higher quality. Never invert comparison conditions (`low_quality = q > quality_to_quantizer(55)`). RDO transform decisions and $4 \times 4$ blocks must stay active for high quality.
   - **Film grain on alpha / graphics:** `estimate_grain` in `src/grain.rs` must be skipped if the image has transparent pixels or flat computer graphics (`px[3] < 255`), avoiding synthesized noise over sharp vector edges.
   - **Auto-subsampling:** Defaults to 4:4:4 for $Q \ge 80$ or alpha imagery; 4:2:0 for $Q < 80$. Because WPG's 16B header eliminates container bloat, 4:4:4 WPG images remain smaller than 4:2:0 AVIF while beating it in PSNR.
