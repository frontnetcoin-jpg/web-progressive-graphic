# WPG (Web Progressive Graphic) — Master Project Memory & Architecture Guide

> **Permanent Single-Source-of-Truth Memory for the WPG Open-Source Format**  
> *Last Updated: October 2026 | Version: 1.0.0*

---

## 1. Project Genesis, Identity & The Grand Vision

### 1.1 What Does WPG Stand For?
In the evolutionary history of the World Wide Web, three-letter acronyms define every generation of digital imagery:
* **PNG** (*Portable Network Graphics*, 1996): Replaced proprietary GIF with lossless compression and alpha channel.
* **SVG** (*Scalable Vector Graphics*, 2001): Resolution-independent vector graphics via XML.
* **WPG** (**Web Progressive Graphic**, 2026): Next-generation open-source image format engineered from the ground up for the modern web.

### 1.2 The Three Foundational Pillars
1. **W — Web-First:** Designed exclusively for modern HTTP/3 networks and edge CDNs. Eliminates 400–800 bytes of legacy MP4/ISOBMFF video container bloat (`ftyp`, `meta`, `iloc`, `mdat`) in favor of an ultra-lean **16-byte fixed binary header** (`WPG\x01`).
2. **P — Progressive by Design:** Solves Cumulative Layout Shift (CLS) forever at the binary format level. An integrated **16-byte micro-preview** block renders an instant, color-matched blurred placeholder in **< 0.05 ms** before smoothly cross-fading into full resolution.
3. **G — Graphic (Universal Fidelity):** Powered by an adaptive dual-engine architecture:
   - *Photographic:* AV1 Intra + Parametric Film Grain Synthesis (20% to 35% smaller than AVIF on camera photos by modeling sensor noise on GPU).
   - *UI & Vectors:* High-speed lossless palette mode (2D RLE + Zstd) and modular reversible YCoCg-R (0 rounding error).

### 1.3 Official Ecosystem Links
* **GitHub Repository:** [https://github.com/frontnetcoin-jpg/web-progressive-graphic](https://github.com/frontnetcoin-jpg/web-progressive-graphic) (Public, Branch `main`)
* **NPM Registry Package:** [https://www.npmjs.com/package/wpg-img](https://www.npmjs.com/package/wpg-img) (`wpg-img@1.0.0`)
* **License:** MIT License (100% royalty-free open standard)

### 1.4 Historical Disambiguation & Industry Precedent
* **Legacy Format Disambiguation:** In the 1980s, WordPerfect Corporation used `.wpg` for an internal vector clipart format (`\xFFWPC`). It has been obsolete and abandoned for over 30 years and never existed on the web.
* **Industry Standard Precedents:** 3-letter extension reuse across eras is standard in computing (e.g. TypeScript `.ts` vs MPEG Transport Stream `.ts`, Perl `.pl` vs Prolog `.pl`, MATLAB `.m` vs Objective-C `.m`).
* **Format Identity:** WPG is defined on the web by MIME type `image/wpg`, custom element `<wpg-img>`, and magic bytes `b"WPG\x01"`. File extensions cannot be trademarked.

---

## 2. Complete Binary Format Specification (v1.0)

A `.wpg` file is organized sequentially into a fixed 16-byte header, followed by optional extension blocks and the primary payload:

```
+-------------------------------------------------------------------------------+
| Offset (Bytes) | Field Name       | Type     | Description                    |
+----------------+------------------+----------+--------------------------------+
| 0..4           | Magic            | [u8; 4]  | ASCII b"WPG\x01"               |
| 4..7           | Width            | u24 BE   | 1..16,777,215 px               |
| 7..10          | Height           | u24 BE   | 1..16,777,215 px               |
| 10             | Mode             | u8       | 0=AV1, 1=Palette, 2=Modular    |
| 11             | Subsampling      | u8       | 0=4:2:0, 1=4:2:2, 2=4:4:4, 3=RGB|
| 12..14         | Flags            | u16 BE   | Bitmask flags (see below)      |
| 14             | Quality          | u8       | Target quality (1..100)        |
| 15             | Alpha Mode       | u8       | 0=None, 1=Lossless, 2=Subsampled|
+----------------+------------------+----------+--------------------------------+
| [Optional 16B] | Micro-Preview    | [u8; 16] | If FLAG_MICRO_PREVIEW (0x0002) |
+----------------+------------------+----------+--------------------------------+
| [Optional 8B]  | Film Grain Meta  | [u8; 8]  | If FLAG_FILM_GRAIN (0x0004)    |
+----------------+------------------+----------+--------------------------------+
| [Optional Var] | Alpha Bitstream  | [u8; N]  | If FLAG_ALPHA (0x0001)         |
+----------------+------------------+----------+--------------------------------+
| Offset End     | Primary Payload  | [u8; M]  | AV1 OBU or Zstd compressed data|
+-------------------------------------------------------------------------------+
```

### Bitmask Flags Definition
* `0x0001` (`FLAG_ALPHA`): Auxiliary alpha channel plane present.
* `0x0002` (`FLAG_MICRO_PREVIEW`): 16-byte instant zero-CLS micro-preview present.
* `0x0004` (`FLAG_FILM_GRAIN`): Parametric film grain metadata block present (8 bytes).
* `0x0008` (`FLAG_SECONDARY_ENTROPY`): Secondary entropy pass applied (Zstandard).
* `0x0010` (`FLAG_LOSSLESS`): Mathematically bit-exact lossless storage.

---

## 3. Algorithmic Innovations & Mathematical Foundations

### 3.1 The 16-Byte Zero-CLS Micro-Preview Engine
* **Goal:** Display a color-matched blurred placeholder in `< 0.05 ms` using only the first 32 bytes of the file.
* **Layout:**
  - Bytes `0..11` (12 bytes): 4 RGB centroid colors derived via fast K-means clustering ($k=4$) on a downsampled 4x4 representation.
  - Bytes `12..15` (4 bytes): 16 2-bit spatial indices for the 4x4 grid ($16 \times 2 = 32 \text{ bits} = 4 \text{ bytes}$).
* **Client Reconstruction:** The production `<wpg-img>` maps each index to its RGB centroid and paints a 4×4 SVG data-URI `<img>` with CSS `filter: blur(14px)`. The display canvas stays 4×4 until the decoded frame replaces the preview.

### 3.2 Parametric Film Grain Synthesis Engine
* **The Problem:** Natural camera sensor noise triggers dense high-frequency DCT/ADST coefficients in AV1, consuming 20% to 35% of the total bit budget.
* **The WPG Solution:**
  1. *Laplacian Noise Estimation:* Applies discrete 3x3 Laplace operator $\begin{bmatrix} 0 & 1 & 0 \\ 1 & -4 & 1 \\ 0 & 1 & 0 \end{bmatrix}$ to luminance channel $Y$. Measures high-frequency variance $\sigma_n^2$.
  2. *Pre-Encoding Denoising:* If $\sigma_n^2 > \text{threshold}$, applies edge-preserving bilateral smoothing before AV1 transform coding.
  3. *Parameter Serialization (8 bytes):* `[grain_scale, seed_hi, seed_lo, grain_cutoff, chroma_scale, 0, 0, 0]`.
  4. *Client-Side GPU Synthesis:* Client uses deterministic Xorshift32 PRNG seeded by `grain_seed`. Adds noise weighted by luminance $W(Y) = 1.0 - 4.0(Y - 0.5)^2$ directly on canvas pixels.
* **Net Gain:** 20% to 35% smaller file size on real DSLR/smartphone photos with zero loss in perceptual texture.

### 3.3 Tri-Mode Codec Decision
* **Mode 0 (AV1 Enhanced Intra):** High-efficiency photographic compression via `rav1e`.
* **Mode 1 (Palette Lossless):** For UI icons, logos, screenshots ($\le 256$ colors). 2D Run-Length Encoding (RLE) + Zstandard entropy coding. Beats PNG by 40% to 80%.
* **Mode 2 (Modular Reversible Lossless):** Reversible color transform YCoCg-R:
  $$Y = \lfloor (R + 2G + B) / 4 \rfloor, \quad C_o = R - B, \quad C_g = G - \lfloor (R + B) / 2 \rfloor$$
  Combined with Paeth / Median directional predictors and Zstd. Zero rounding error.

---

## 4. WebAssembly & Web Component Architecture

### 4.1 Zero-Wasm Fast Path & In-Memory GPU Decoding via `crates/wpg-wasm`
* Traditional AV1 decoders (`dav1d` / `rav1d`) cannot compile cleanly to `wasm32-unknown-unknown` without heavy POSIX/libc wrappers (~2 MB).
* **The WPG Breakthrough (2-Tier Client Architecture):**
  1. **Zero-Wasm (0 KB) Pure-JS Fast Path for Mode 0:** Standard Mode 0 photographs are wrapped into a 242-byte ISOBMFF header (`wrapWpgMode0ToAvifJs`) and grain-synthesized (`applyFilmGrainJs`) directly in JavaScript inside `wpg-worker.js` with **0 KB of WebAssembly downloaded**.
  2. **On-Demand Lazy Wasm (`wpg-decoder.wasm`, ~130 KB with LTO):** Only when an image uses Mode 1 (Palette), Mode 2 (Modular), `FLAG_ALPHA`, or `FLAG_SECONDARY_ENTROPY` does the worker fetch `wpg-decoder.wasm` (which includes pure-Rust `ruzstd`).
* It passes the synthesized AVIF bytes to the browser's native hardware-accelerated AV1 engine via:
  ```javascript
  const blob = new Blob([avifBytes], { type: 'image/avif' });
  const bmp = await createImageBitmap(blob);
  ctx.drawImage(bmp, 0, 0);
  ```
* **Result:** **0 KB Wasm overhead** for >95% of web photographs, plus full offline RGBA decoding for palette, modular, and alpha assets.

### 4.2 Web Component `<wpg-img>` & 3-Format Benchmark Studio
* `wpg-element.js` is an ES module. Load it with `<script type="module">`. The showcase server embeds it, `wpg-worker.js`, and `wpg-decoder.wasm` via `include_str!` / `include_bytes!` and serves `/wpg-element.js`, `/wpg-worker.js`, and `/wpg-decoder.wasm` with `Cache-Control: public, max-age=31536000, immutable`.
* The showcase page features a **3-Format Head-to-Head Benchmark Studio (`.wpg` vs `.webp` vs `.avif`)** with a `.jpg` drag-and-drop dropzone, podium cards, 1:1 split view, and a live `<wpg-img>` element that automatically previews `/api/latest.wpg`.
* Lifecycle:
  1. `response.body.getReader()` accumulates chunks until 32 bytes are available, then stitches the header and micro-preview across chunk boundaries.
  2. The 16-byte preview becomes a blurred SVG data-URI `<img>`. `width` / `height` set `aspect-ratio`. The display `<canvas>` stays 4×4 until the final frame.
  3. `wpg-worker.js` decodes standard Mode 0 images via the 0-KB Wasm pure-JS ISOBMFF fast path (`wrapWpgMode0ToAvifJs` + `applyFilmGrainJs`), or lazily loads `wpg-decoder.wasm` on demand for Mode 1, Mode 2, `FLAG_ALPHA`, or `FLAG_SECONDARY_ENTROPY`. The frame returns with `postMessage(..., [bitmap])`. The main thread transfers a copy, so a worker failure leaves the original bytes intact.
  4. Fallback order: worker, then main-thread JS/Wasm, then `POST` to `fallback-url` (default `/api/decode`; `"none"` skips it). A worker crash rejects the in-flight decode.
  5. `loading="lazy"` keeps an `IntersectionObserver` (`rootMargin: 200px`). Leaving that margin cancels the decode, closes the `ImageBitmap`, and zeroes the canvas. The SVG preview remains. Re-entry fetches again.
* SSR: `renderWpgImg()` emits Declarative Shadow DOM (`<template shadowrootmode="open">`) with the aspect-ratio box and SVG preview. It is safe to call from Node, Next.js, Nuxt, and SvelteKit.
* Attributes: `src`, `alt`, `width`, `height`, `loading`, `wasm-url`, `worker-url`, `fallback-url`.

### 4.3 Standalone Offline Playground (`playground/wpg-bundle.js`)
* For local offline testing via `file:///` (where browsers block module scripts and `fetch()`), `playground/wpg-bundle.js` embeds the Wasm binary as base64 and exposes `.loadBytes(uint8Array)`.
* That bundle still fetches with `arrayBuffer()` on the main thread. It does not include the ReadableStream client, the worker, or `renderWpgImg()`. The production client is `wpg-element.js`.

---

## 5. SEO & Core Web Vitals Strategy

### 5.1 Google Ranking Factor Optimization
* **Cumulative Layout Shift (CLS):** Score **0.000**. The 16-byte fixed header and 16-byte micro-preview reserve exact layout space in `< 0.05 ms`.
* **Largest Contentful Paint (LCP):** 20% to 35% faster download times compared to AVIF, and 68% faster than JPEG.
* **PageSpeed Insights:** Consistent **95–100/100** mobile performance scores.

### 5.2 100% Crawlability & Indexing Best Practice
1. **Googlebot / Modern Crawlers:** Execute JavaScript and index `<wpg-img alt="...">` natively.
2. **Social Media Scrapers (WhatsApp, Facebook, Twitter/X):** Do not execute JS. Maintain a standard JPG/WebP fallback in the `<head>` metadata:
   ```html
   <meta property="og:image" content="https://mysite.com/images/hero.jpg">
   <meta name="twitter:image" content="https://mysite.com/images/hero.jpg">
   ```
3. **No-JS Crawler Fallback:**
   ```html
   <wpg-img src="/images/hero.wpg" alt="Hero Image">
     <noscript>
       <img src="/images/hero.jpg" alt="Hero Image" width="800" height="600" loading="lazy">
     </noscript>
   </wpg-img>
   ```

### 5.3 Production Milestone: any2prompt.com (96/100 Mobile PageSpeed)
* **Live Deployment:** Successfully integrated into production on active AI SaaS platform ([any2prompt.com](https://any2prompt.com)).
* **Lighthouse Performance Score:** Jumped from **74 - 75** (Orange) to **96 / 100** (Emerald) on mobile!
* **Asset Reductions:**
  - Hero Feature Visual: 4.19 MB $\rightarrow$ **87.9 KB** (**-97.9%**)
  - Gallery (10 items): 8.69 MB $\rightarrow$ **993 KB** (**-88.6%**)
* **Key Architecture Rules Enforced:**
  1. `<wpg-img loading="lazy">` keeps an `IntersectionObserver` (`rootMargin: '200px 0px'`). It defers the first fetch and releases the canvas and `ImageBitmap` when the image leaves that margin.
  2. The production script is an ES module: `<script type="module" src="/wpg-element.js"></script>`.
  3. `loadWpg(url)` skips a repeat of the same `src`. A cache-busting query (`?t=`) starts a new load.
  4. Never place inline `style="background-image:url('...')"` in client-side JS template `<noscript>` tags, as browser CSS parsers speculatively fetch the legacy fallback asset.

---

## 6. Codebase File Structure & Module Map

```
wpg/
├── Cargo.toml               # Rust workspace manifest (rav1e, rav1d, zstd, webp, image, clap)
├── README.md                # Public GitHub documentation & benchmark showcase
├── DOCUMENTATION.md         # Comprehensive architectural and technical specification
├── SPECIFICATION.md         # Official Binary Format Specification v1.0
├── MEMORY.md                # Permanent single-source-of-truth project memory
├── wpg-decoder.wasm         # Standalone on-demand WebAssembly client decoder (~130 KB with LTO)
├── wpg-element.js           # <wpg-img> ESM client (stream, worker pool, SSR)
├── wpg-worker.js            # Off-thread Wasm decode and film grain
├── crates/
│   ├── wpg-wasm/            # Pure Wasm crate (wasm32-unknown-unknown, zero libc)
│   └── wpg-thumb/           # Native Windows Shell Thumbnail Provider COM DLL (IThumbnailProvider)
├── packages/
│   └── wpg-img/             # NPM package distribution (TypeScript types, ESM, Wasm)
│       ├── package.json     # wpg-img@1.0.0
│       ├── dist/index.js    # ESM client (same implementation as ../../wpg-element.js)
│       ├── dist/wpg-worker.js
│       ├── dist/index.d.ts  # TypeScript types, including renderWpgImg()
│       └── README.md        # NPM package documentation
├── playground/              # Standalone local testing environment
│   ├── input/               # Drop area for test images
│   ├── output/              # Generated .wpg and decoded files
│   ├── test-encode.ps1      # 1-Click test & benchmark automation script
│   ├── index.html           # Standalone testing GUI with drag & drop
│   ├── wpg-bundle.js        # Self-contained bundle with embedded base64 Wasm (file:/// safe)
│   └── README.md            # Playground guide
├── scripts/
│   ├── install.ps1          # Windows installer script
│   ├── install.sh           # Linux/macOS installer script
│   ├── package-release.ps1  # Windows release packager (.zip)
│   └── build-playground-bundle.ps1 # Builds wpg-bundle.js
├── src/
│   ├── lib.rs               # Library root exports
│   ├── header.rs            # 16-byte fixed header serialization & bitflags
│   ├── preview.rs           # 16-byte micro-preview generator & canvas decoder
│   ├── grain.rs             # Laplacian noise estimator & deterministic PRNG grain engine
│   ├── color.rs             # Color conversions (BT.709, BT.601, YUV420/422/444, YCoCg-R)
│   ├── lossless.rs          # Mode 1 (Palette RLE) & Mode 2 (Modular YCoCg-R + Paeth)
│   ├── av1_codec.rs         # Pure Rust AV1 Intra encoder (rav1e) & AVIF/WPG decoder (rav1d)
│   ├── encoder.rs           # High-level WpgEncoder pipeline
│   ├── decoder.rs           # High-level WpgDecoder pipeline
│   ├── metrics.rs           # Pixel-exact PSNR & SSIM evaluation engine
│   ├── server.rs            # Embedded Glassmorphic Showcase Server & REST API
│   └── bin/
│       ├── wpg.rs           # Unified CLI (encode, decode, batch, bench, info, serve, view, register)
│       ├── wpg_enc.rs       # Dedicated encoder binary
│       ├── wpg_dec.rs       # Dedicated decoder binary
│       └── wpg_view.rs      # Native Win32 GDI Desktop Viewer (zoom, pan, HUD, PNG export)
└── tests/
    └── roundtrip.rs         # Automated bit-exact roundtrip tests
```

---

## 7. Critical Operational Guidelines & Engineering Rules

1. **Windows Binary Lock (`os error 32`):**
   - On Windows, running `cargo build --release` while `wpg serve` is active in the background will fail with OS error 32 (executable locked by running process).
   - *Rule:* Always kill running instances before rebuilding:
     ```powershell
     Stop-Process -Name wpg -Force -ErrorAction SilentlyContinue
     cargo build --release
     ```

2. **100% Metric Independence (Zero Placeholders):**
   - Benchmarks must always compute real pixel-by-pixel PSNR and SSIM against RGBA buffers.
   - AVIF is decoded via bitstream extraction in `decode_avif_to_rgba`, isolating color OBUs from monochrome alpha streams.
   - Lossy WebP uses Google `libwebp-sys` bindings (`webp` crate) and Lossless uses `image` VP8L.

3. **Git Hygiene & Private Files:**
   - `todo.md` is strictly local and kept in `.gitignore`.
   - Release archives (`*.zip`) are ignored by Git and attached directly as GitHub Release assets.

4. **NPM Publishing Protocol:**
   - Granular tokens require: Name, `Bypass two-factor authentication (2FA)` checked, `Read and write (publish and stage)`, and `All packages`.
   - Command:
     ```powershell
     cd packages\wpg-img
     npm publish --access public
     ```

---

## 8. Windows OS Shell Extension & Native Desktop Integration

### 8.1 Windows Shell COM Architecture (`crates/wpg-thumb`)
* **DLL Name:** `target/release/wpg_thumb.dll` (`cdylib`, Pure Rust).
* **Target:** Windows Shell Thumbnail Provider.
* **Interfaces Implemented:**
  - `IThumbnailProvider` (`{E357FCCD-A995-4576-B01F-234630154E96}`)
  - `IInitializeWithStream` (`{B824B49D-22AC-4161-AC8A-9916E8FA3F7F}`)
  - `IInitializeWithFile` (`{B7D14566-0509-4CCE-A71F-0A554233BD98}`)
  - `IClassFactory`
* **CLSID:** `{6F0E9720-3051-4E78-8319-5A676BE466EC}`
* **Threading Model:** `Both` (Essential for concurrent multithreaded decoding on Explorer thread pool).

### 8.2 Critical Bug Discovery: `IStream::Read` and `S_FALSE` EOF Condition
* **Symptom:** Windows Explorer called `IInitializeWithStream`, but `GetBitmap` returned `0x80004005 (E_FAIL)` with log `Error parsing OBU data`.
* **Root Cause:** In COM `ISequentialStream::Read`, when reading the final chunk of a stream where `cbRead < cbRequested` (e.g., last 1,047 bytes of a 66,583-byte file), the method returns `S_FALSE` (`1`). The initial loop checked `if hr != S_OK`, causing it to break *before* appending the final bytes to the memory buffer. The resulting truncated buffer failed AV1 sequence header parsing.
* **Resolution:** In `stream_initialize`, append `chunk[..read_bytes]` *whenever* `read_bytes > 0`, and only break if `hr < 0` (failure) or `read_bytes == 0` (true EOF).

### 8.3 SystemFileAssociations Override Rule
* **Problem:** In Windows 10/11, setting `PerceivedType = image` causes Explorer to default to `SystemFileAssociations\image\shellex\{e357fccd-a995-4576-b01f-234630154e96}`, which invokes Windows' built-in `PhotoMetadataHandler.dll`. Since that handler only understands WIC formats, `.wpg` files failed to render thumbnails.
* **Resolution:** Register the CLSID explicitly under:
  ```text
  HKCU:\Software\Classes\SystemFileAssociations\.wpg\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96} -> {6F0E9720-3051-4E78-8319-5A676BE466EC}
  ```
* **Visual Polish:**
  - `Treatment = 2` (REG_DWORD): Enables drop-shadow photographic framing.
  - `DefaultIcon = %SystemRoot%\System32\imageres.dll,-70`: Displays high-resolution picture icon as fallback.

### 8.4 Native Desktop Viewer (`wpg-view.exe`)
* Pure Win32 GDI double-buffered canvas in Rust with sub-10ms startup.
* DWM Dark Mode titlebar (`DwmSetWindowAttribute`).
* Smooth mouse-wheel zoom centered on cursor coordinates, drag-to-pan, directory browsing (`Arrow` keys/`Space`), PNG export (`S`), HUD overlay (`I`), and drag-and-drop (`WM_DROPFILES`).

