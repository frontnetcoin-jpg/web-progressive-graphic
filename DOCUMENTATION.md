# WPG (Web Progressive Graphic) — Comprehensive Technical Documentation v1.0

---

## Table of Contents
1. [Introduction & Design Philosophy](#1-introduction--design-philosophy)
2. [Binary Format Specification](#2-binary-format-specification)
   - [2.1 Fixed 16-Byte Header](#21-fixed-16-byte-header)
   - [2.2 Header Flags](#22-header-flags)
   - [2.3 Optional Payload Blocks](#23-optional-payload-blocks)
3. [16-Byte Zero-CLS Micro-Preview](#3-16-byte-zero-cls-micro-preview)
   - [3.1 Extraction & K-Means Quantization](#31-extraction--k-means-quantization)
   - [3.2 2-Bit Spatial Bit-Packing](#32-2-bit-spatial-bit-packing)
   - [3.3 Client-Side Reconstruction & Blur](#33-client-side-reconstruction--blur)
4. [Film Grain Synthesis Engine](#4-film-grain-synthesis-engine)
   - [4.1 Noise Detection & Variance Estimation](#41-noise-detection--variance-estimation)
   - [4.2 Pre-Encoding Bilateral Denoising](#42-pre-encoding-bilateral-denoising)
   - [4.3 Deterministic PRNG Grain Synthesis](#43-deterministic-prng-grain-synthesis)
5. [Codec Modes & Color Spaces](#5-codec-modes--color-spaces)
   - [5.1 Mode 0: AV1 Enhanced Intra](#51-mode-0-av1-enhanced-intra)
   - [5.2 Mode 1: Palette Lossless (UI / Vector)](#52-mode-1-palette-lossless-ui--vector)
   - [5.3 Mode 2: Modular Lossless (Reversible YCoCg-R)](#53-mode-2-modular-lossless-reversible-ycocg-r)
   - [5.4 Color Transforms & Subsampling](#54-color-transforms--subsampling)
6. [Rust API Reference](#6-rust-api-reference)
   - [6.1 WpgEncoder](#61-wpgencoder)
   - [6.2 WpgDecoder](#62-wpgdecoder)
   - [6.3 WpgHeader & Config Types](#63-wpgheader--config-types)
7. [CLI Tools & Workflows](#7-cli-tools--workflows)
   - [7.1 encode / decode](#71-encode--decode)
   - [7.2 batch (Directory Conversion)](#72-batch-directory-conversion)
   - [7.3 bench & info](#73-bench--info)
   - [7.4 serve (Showcase & Viewer)](#74-serve-showcase--viewer)
8. [Web Component `<wpg-img>`](#8-web-component-wpg-img)
   - [8.1 Lifecycle & Progressive Fetch](#81-lifecycle--progressive-fetch)
   - [8.2 REST Endpoints](#82-rest-endpoints)
9. [Verification, Benchmarking & Metrics](#9-verification-benchmarking--metrics)
   - [9.1 Metric Definitions (PSNR & SSIM)](#91-metric-definitions-psnr--ssim)
   - [9.2 True AVIF Bitstream Decoding](#92-true-avif-bitstream-decoding)
10. [Client-Side WebAssembly & NPM Distribution](#10-client-side-webassembly--npm-distribution)
    - [10.1 WebAssembly Client-Side Decoder (`wpg-decoder.wasm`)](#101-webassembly-client-side-decoder-wpg-decoderwasm)
    - [10.2 NPM Package (`wpg-img`)](#102-npm-package-wpg-img)
    - [10.3 Standalone Binary Distribution](#103-standalone-binary-distribution)
11. [SEO & Core Web Vitals Engineering](#11-seo--core-web-vitals-engineering)
    - [11.1 Cumulative Layout Shift (CLS: 0.000)](#111-cumulative-layout-shift-cls-0000)
    - [11.2 Largest Contentful Paint (LCP)](#112-largest-contentful-paint-lcp)
    - [11.3 Crawler Indexing & Fallbacks](#113-crawler-indexing--fallbacks)
    - [11.4 Production Case Study (any2prompt.com: 96/100 Mobile PageSpeed)](#114-production-case-study-any2promptcom-96100-mobile-pagespeed)
    - [11.5 Performance Architecture: Lazy Loading & Script Defer](#115-performance-architecture-lazy-loading--script-defer)
12. [Local Testing Playground (`playground/`)](#12-local-testing-playground-playground)

---

## 1. Introduction & Design Philosophy

The **Web Progressive Graphic (.wpg)** is a royalty-free image format engineered specifically for high-efficiency image distribution on the modern web.

While **AVIF** demonstrated the power of AV1 video intra-frames for still images, its adherence to the **ISOBMFF (HEIF / ISO/IEC 14496-12)** container standard imposes severe penalties:
- **Container Bloat:** Nested boxes (`ftyp`, `meta`, `iloc`, `iinf`, `ipco`, `ipma`, `mdat`) impose **400 to 800 bytes** of fixed overhead per image, creating high relative inefficiency for thumbnails, icons, and avatars.
- **Sensor Noise Tax:** Camera sensor noise and natural grain trigger dense high-frequency DCT/ADST coefficients, consuming 20% to 35% of the total bit budget.
- **Layout Instability:** Web applications cannot display sub-millisecond, low-latency previews without downloading full image metadata.

**WPG solves these problems by design:**
1. A **16-byte fixed binary header** replaces all ISOBMFF container boxes.
2. An embedded **16-byte micro-preview** allows browsers to render instant placeholders in **< 0.05 ms**, completely eliminating Cumulative Layout Shift (CLS).
3. A **parametric film grain engine** estimates high-frequency noise, denoises the base frame prior to AV1 transform coding, and synthesizes natural grain deterministically on the client.
4. An **adaptive tri-mode engine** selects between AV1 intra-coding for photographs, palette-based 2D RLE for graphics, and reversible YCoCg-R for bit-exact lossless storage.

### 1.1 Historical Disambiguation
*Web Progressive Graphic* (`.wpg`) is a 2026 web standard designed exclusively for modern HTTP/3 networks and edge delivery. It bears **zero relation** to the legacy 1980s *WordPerfect Graphics* (`.wpg`) vector clipboard format developed for DOS/early Windows office documents. In standard computing practice (such as TypeScript `.ts` vs MPEG Transport Stream `.ts`), file extensions are disambiguated by their domain, MIME type (`image/wpg`), and magic byte headers (`WPG\x01` vs `\xFFWPC`).

---

## 2. Binary Format Specification

A `.wpg` file is organized sequentially into a fixed 16-byte header followed by optional extensions and the primary bitstream payload:

```
+--------------------------------------------------------------------------+
| Fixed Header (16 Bytes)                                                  |
| Offset 0..4   : Magic (b"WPG\x01")                                       |
| Offset 4..7   : Width (u24 Big-Endian, 1..16,777,215)                    |
| Offset 7..10  : Height (u24 Big-Endian, 1..16,777,215)                   |
| Offset 10     : Mode (u8: 0=AV1, 1=Palette, 2=Modular)                   |
| Offset 11     : Subsampling (u8: 0=4:2:0, 1=4:2:2, 2=4:4:4, 3=RGB)       |
| Offset 12..14 : Flags (u16 Big-Endian)                                   |
| Offset 14     : Quality (u8: 1..100)                                     |
| Offset 15     : Alpha Mode (u8: 0=None, 1=Lossless Plane, 2=Subsampled)  |
+--------------------------------------------------------------------------+
| [Optional] Micro-Preview Block (16 Bytes, if FLAG_MICRO_PREVIEW set)     |
| - 12 bytes : 4 dominant RGB colors (4 x 3 bytes)                         |
| - 4 bytes  : 16 2-bit spatial palette indices (4x4 grid)                 |
+--------------------------------------------------------------------------+
| [Optional] Film Grain Synthesis Block (8 Bytes, if FLAG_FILM_GRAIN set)  |
| - 1 byte  : grain_scale (u8, 0..255)                                     |
| - 2 bytes : grain_seed (u16 Big-Endian PRNG seed)                        |
| - 1 byte  : grain_cutoff (u8 frequency cutoff)                           |
| - 1 byte  : chroma_scale (u8 modulation)                                 |
| - 3 bytes : reserved padding                                             |
+--------------------------------------------------------------------------+
| [Optional] Alpha Plane Bitstream (Variable length, if FLAG_ALPHA set)    |
| - 4 bytes : alpha_length (u32 Big-Endian)                                |
| - N bytes : Zstandard-compressed monochrome alpha stream                 |
+--------------------------------------------------------------------------+
| Primary Bitstream Payload (Remaining Bytes to End-of-File)                |
| - Mode 0 : Raw AV1 Open Bitstream Unit (OBU) Intra Stream                |
| - Mode 1 : Palette Color Table + 2D RLE Index Stream (Zstd compressed)   |
| - Mode 2 : Reversible YCoCg-R Residuals + Paeth Predictors (Zstd comp.)  |
+--------------------------------------------------------------------------+
```

### 2.1 Fixed 16-Byte Header

```rust
pub struct WpgHeader {
    pub magic: [u8; 4],      // b"WPG\x01"
    pub width: u32,          // 24-bit unsigned integer (max 16,777,215 px)
    pub height: u32,         // 24-bit unsigned integer (max 16,777,215 px)
    pub mode: WpgMode,       // Mode 0, 1, or 2
    pub subsampling: ColorSubsampling, // 4:2:0, 4:2:2, 4:4:4, RGB
    pub flags: u16,          // Bitfield options
    pub quality: u8,         // Quality scale (1..100)
    pub alpha_mode: u8,      // Alpha encoding strategy
}
```

### 2.2 Header Flags

| Constant | Value | Description |
|---|---|---|
| `FLAG_ALPHA` | `0x0001` | Alpha channel stream is present in container |
| `FLAG_MICRO_PREVIEW` | `0x0002` | 16-byte micro-preview block is present directly after header |
| `FLAG_FILM_GRAIN` | `0x0004` | 8-byte parametric film grain synthesis metadata block is present |
| `FLAG_SECONDARY_ENTROPY` | `0x0008` | Secondary Zstandard finite-state entropy wrapping is active |
| `FLAG_LOSSLESS` | `0x0010` | Lossless bit-exact reconstruction mode |

---

## 3. 16-Byte Zero-CLS Micro-Preview

### 3.1 Extraction & K-Means Quantization
During encoding, the image is scaled to a $4 \times 4$ spatial matrix (16 pixels). K-means clustering ($k=4$) calculates four dominant centroid colors:
$$\mathbf{C}_0, \mathbf{C}_1, \mathbf{C}_2, \mathbf{C}_3 \in [0, 255]^3$$
These 4 RGB triples are serialized directly into **12 bytes**:
$$[\, R_0, G_0, B_0, \; R_1, G_1, B_1, \; R_2, G_2, B_2, \; R_3, G_3, B_3 \,]$$

### 3.2 2-Bit Spatial Bit-Packing
Each of the 16 pixels is matched to its closest centroid using Euclidean color distance:
$$idx(x, y) = \arg\min_{i \in \{0, 1, 2, 3\}} \|\mathbf{P}(x, y) - \mathbf{C}_i\|^2$$
The 16 2-bit indices ($16 \times 2 = 32 \text{ bits} = 4 \text{ bytes}$) are bit-packed:
$$\text{Byte } n = (idx_{4n} \ll 6) \mid (idx_{4n+1} \ll 4) \mid (idx_{4n+2} \ll 2) \mid idx_{4n+3}$$
The entire block requires **exactly 16 bytes**.

### 3.3 Client-Side Reconstruction & Blur
A web browser or CLI decoder unpacks the 16 bytes into a $4 \times 4$ canvas in **under 0.05 milliseconds**:
```javascript
const canvas = document.createElement('canvas');
canvas.width = 4; canvas.height = 4;
const ctx = canvas.getContext('2d');
// Paint 16 pixels using unpacked palette...
canvas.style.filter = 'blur(20px)';
canvas.style.transform = 'scale(1.05)'; // eliminates edge blur artifacts
```
This produces an immediate, visually accurate placeholder with **zero layout shifts**.

---

## 4. Film Grain Synthesis Engine

### 4.1 Noise Detection & Variance Estimation
Natural camera noise is modeled as an additive high-frequency signal $N(x, y)$ superimposed on the true luminance signal $I(x, y)$:
$$\hat{N}(x, y) = I(x, y) \ast \begin{bmatrix} 0 & -1 & 0 \\ -1 & 4 & -1 \\ 0 & -1 & 0 \end{bmatrix}$$
The noise variance $\sigma_N^2$ is computed across flat image patches (excluding high-gradient edges) to derive the parameter `grain_scale`:
$$\text{grain\_scale} = \min\left(255, \left\lfloor \frac{\sigma_N}{\sigma_{\max}} \times 255 \right\rfloor\right)$$

### 4.2 Pre-Encoding Bilateral Denoising
Before feeding pixels to the AV1 encoder, an edge-preserving bilateral filter smooths the flat regions:
$$I_{\text{denoised}}(p) = \frac{1}{W_p} \sum_{q \in \Omega} I(q) \, G_{\sigma_s}(\|p - q\|) \, G_{\sigma_r}(|I(p) - I(q)|)$$
Because high-frequency sensor noise is eliminated from the DCT transform blocks, the AV1 intra bitstream size drops by **20% to 35%**.

### 4.3 Deterministic PRNG Grain Synthesis
Upon decoding, a high-speed Xorshift32 PRNG initialized with `grain_seed` generates Gaussian-distributed noise $\epsilon \sim \mathcal{N}(0, 1)$. The noise is luminance-modulated to avoid artifacts in extreme shadows and highlights:
$$M(Y) = 1.0 - 4.0 \cdot \left(\frac{Y}{255.0} - 0.5\right)^2$$
$$Y_{\text{final}} = \text{clamp}\left(Y + \epsilon \cdot \text{grain\_scale} \cdot M(Y), 0, 255\right)$$

---

## 5. Codec Modes & Color Spaces

### 5.1 Mode 0: AV1 Enhanced Intra
Mode 0 packages a raw AV1 Open Bitstream Unit (OBU) sequence intra-frame.
- **Quantizer Calibration:** Standard AVIF uses an exponential quantizer mapping. WPG uses a calibrated logarithmic mapping curve:
  $$\text{quantizer}(Q) = \text{round}\left(255 \cdot \left(1.0 - \frac{Q}{100.0}\right)^{1.35}\right)$$
  This aligns WPG quality scores with industry-standard visual expectations.
- **Pure Rust AV1 Core:** Encoded via `rav1e` and decoded via `rav1d` with zero external C dependencies.

### 5.2 Mode 1: Palette Lossless (UI / Vector)
Designed for screenshots, charts, UI mockups, icons, and vector graphics containing $\le 256$ unique colors:
1. Palette extraction identifies unique 32-bit RGBA entries.
2. The image is mapped to an 8-bit index buffer.
3. A 2D Run-Length Encoder (RLE) compresses contiguous horizontal runs of identical colors.
4. Zstandard level 19 super-compression packs the stream.
*Produces files up to 80% smaller than standard PNG.*

### 5.3 Mode 2: Modular Lossless (Reversible YCoCg-R)
For continuous-tone photographs requiring 100% bit-exact lossless preservation, WPG employs the reversible YCoCg-R color transform:
$$\begin{aligned}
Co &= R - B \\
t &= B + \lfloor Co / 2 \rfloor \\
Cg &= G - t \\
Y &= t + \lfloor Cg / 2 \rfloor
\end{aligned}$$
**Exact Inverse:**
$$\begin{aligned}
t &= Y - \lfloor Cg / 2 \rfloor \\
G &= Cg + t \\
B &= t - \lfloor Co / 2 \rfloor \\
R &= B + Co
\end{aligned}$$
Residuals are predicted using the **Paeth / Median Edge Detector (MED)** and compressed using Zstandard.

### 5.4 Color Transforms & Subsampling
- **BT.709:** Standard for HD/Web content.
- **BT.601:** Used for compatibility with legacy AVIF decoders.
- **Subsampling Modes:**
  - `YUV 4:2:0` (Default for general photos, half horizontal and vertical chroma resolution).
  - `YUV 4:2:2` (High-fidelity color, half horizontal chroma resolution).
  - `YUV 4:4:4` (Full chroma resolution, recommended for sharp text).
  - `RGB` (Direct color channel storage).

---

## 6. Rust API Reference

### 6.1 WpgEncoder
```rust
use wpg::{WpgEncoder, WpgOptions, WpgMode, ColorSubsampling};

let options = WpgOptions {
    quality: 80,
    speed: 6,
    mode: WpgMode::Av1Enhanced,
    subsampling: ColorSubsampling::Yuv420,
    enable_micro_preview: true,
    enable_film_grain: true,
};

let encoder = WpgEncoder::new(options);
let wpg_bytes: Vec<u8> = encoder.encode_rgba(width, height, &rgba_data)?;
```

### 6.2 WpgDecoder
```rust
use wpg::WpgDecoder;

let decoder = WpgDecoder::new();

// Full decoding
let decoded = decoder.decode(&wpg_bytes)?;
println!("Decoded: {}x{}, {} pixels", decoded.width, decoded.height, decoded.rgba.len() / 4);

// Instant micro-preview decoding (< 0.05 ms)
let preview = decoder.decode_preview_only(&wpg_bytes)?;
// preview.rgba is a 4x4 RGBA buffer (64 bytes)
```

### 6.3 WpgHeader & Config Types
```rust
use wpg::WpgHeader;

let header = WpgHeader::from_bytes(&wpg_bytes[..16])?;
println!("Dimensions: {}x{}, Mode: {:?}", header.width, header.height, header.mode);
```

---

## 7. CLI Tools & Workflows

### 7.1 encode / decode
```powershell
# High quality encode
.\target\release\wpg.exe encode input.png output.wpg -q 85 -s 5

# Decode to PNG
.\target\release\wpg.exe decode output.wpg restored.png

# Extract micro-preview placeholder
.\target\release\wpg.exe decode output.wpg placeholder.png --preview-only
```

### 7.2 batch (Directory Conversion)
```powershell
# Batch convert an entire folder of photos
.\target\release\wpg.exe batch C:\Users\user\Photos -o C:\Users\user\WebPhotos -q 75 -s 6 -r
```

### 7.3 bench & info
```powershell
# Inspect container structure and grain parameters
.\target\release\wpg.exe info output.wpg

# Run side-by-side PSNR, SSIM, and bpp benchmark against PNG, JPEG, and WebP
.\target\release\wpg.exe bench sample.png --quality 80
```

### 7.4 serve (Showcase & Viewer)
```powershell
.\target\release\wpg.exe serve --port 3000
```
Launches the embedded HTTP server with the interactive split-slider GUI, zoom lens, live parameter controls, and REST API.

---

## 8. Web Component `<wpg-img>`

### 8.1 Lifecycle & Progressive Fetch
The `wpg-element.js` script registers a custom Web Component:
```html
<script src="http://localhost:3000/wpg-element.js"></script>
<wpg-img src="sample.wpg" alt="Description"></wpg-img>
```

1. **ConnectedCallback:** Initializes Shadow DOM with an embedded `<canvas>` and an `<img>` tag.
2. **Phase 1 (Stream Slice 32 Bytes):** Reads the 16-byte header and 16-byte preview block. The canvas renders the 4x4 preview with `filter: blur(20px)` immediately, guaranteeing **0 Cumulative Layout Shift (CLS)**.
3. **Phase 2 (Payload Stream & Decode):** Streams the full file and calls the decode endpoint or local Wasm worker.
4. **Phase 3 (Cross-Fade):** Smoothly transitions opacity from canvas to full image.

### 8.2 REST Endpoints
The `wpg serve` background service exposes:
- `POST /api/encode`: Accepts multipart image file and query params (`quality`, `speed`, `grain`, `mode`), returns benchmark metrics and encoded `.wpg`.
- `POST /api/decode`: Accepts raw `.wpg` bytes, returns decoded PNG.
- `GET /api/sample.wpg`: Serves a reference `.wpg` file.
- `GET /wpg-element.js`: Serves the Web Component script.

---

## 9. Verification, Benchmarking & Metrics

### 9.1 Metric Definitions (PSNR & SSIM)
- **PSNR (Peak Signal-to-Noise Ratio):**
  $$\text{MSE} = \frac{1}{3WH} \sum_{c \in \{R,G,B\}} \sum_{x, y} \left(I_1(x, y, c) - I_2(x, y, c)\right)^2$$
  $$\text{PSNR} = 10 \cdot \log_{10}\left(\frac{255^2}{\text{MSE}}\right)$$
- **SSIM (Structural Similarity Index Measure):**
  $$\text{SSIM}(x, y) = \frac{(2\mu_x\mu_y + C_1)(2\sigma_{xy} + C_2)}{(\mu_x^2 + \mu_y^2 + C_1)(\sigma_x^2 + \sigma_y^2 + C_2)}$$

### 9.2 True AVIF & Lossy WebP Bitstream Decoding
To ensure 100% confidence in comparative benchmarks:
- **AVIF Decoupling:** The standard `ravif` encoder writes an ISOBMFF file with an `mdat` container. For RGBA inputs, `ravif` stores an auxiliary monochrome 4:0:0 alpha stream first, followed by the color AV1 OBU stream. WPG's `decode_avif_to_rgba` inspects the `mdat` box, skips the alpha payload, feeds the color OBU to `rav1d`, and applies Rec.601 matrix conversion to guarantee authentic, independent PSNR and SSIM calculation.
- **Lossy WebP Integration:** Lossy WebP is encoded using the official Google `libwebp-sys` bindings (`webp` crate) and decoded back to RGBA for pixel-exact PSNR and SSIM computation, giving a fair comparison against AV1 and WPG.

---

## 10. Client-Side WebAssembly & NPM Distribution

### 10.1 WebAssembly Client-Side Decoder (`wpg-decoder.wasm`)
The `crates/wpg-wasm` crate compiles down to a lightweight 61 KB binary (`wpg-decoder.wasm`) without POSIX/libc dependencies:
- `wpg_parse_info(ptr, len, out_info)`: Extracts dimensions, mode, and bitflags in < 0.01 ms.
- `wpg_decode_preview(ptr, len, out_rgba_64)`: Unpacks the 16-byte micro-preview into a 4x4 RGBA buffer.
- `wpg_wrap_to_avif(ptr, len, out_ptr, out_len)`: Wraps the raw AV1 OBU payload in a minimal ISOBMFF header directly in WebAssembly memory, allowing browsers to invoke native hardware AV1 video decoders via `URL.createObjectURL(new Blob([avifBytes], { type: 'image/avif' }))`.
- `wpg_apply_film_grain(rgba_ptr, w, h, scale, seed)`: Synthesizes high-frequency noise deterministically in real-time across canvas pixels.

### 10.2 NPM Package (`wpg-img`)
Located in `packages/wpg-img/`, this package exposes the custom `<wpg-img>` element along with programmatic TypeScript APIs for React, Vue, Svelte, Nuxt, and Next.js:
```bash
npm install wpg-img
```

### 10.3 Standalone Binary Distribution
- **Windows:** `scripts/install.ps1` downloads and registers `wpg.exe` into user PATH.
- **Linux/macOS:** `scripts/install.sh` installs the binary to `~/.local/bin/wpg`.
- **Packaging:** `scripts/package-release.ps1` creates `wpg-v1.0.0-windows-x86_64.zip`.

---

## 11. SEO & Core Web Vitals Engineering

### 11.1 Cumulative Layout Shift (CLS: 0.000)
Core Web Vitals heavily penalize cumulative layout shifts. The 16-byte fixed container header combined with the 16-byte micro-preview guarantees that `<wpg-img>` computes the exact aspect ratio in `< 0.05 ms`, allocating canvas boundaries before any AV1 payload data is fetched. This guarantees a theoretical and practical **CLS score of 0.000**.

### 11.2 Largest Contentful Paint (LCP)
With a 20% to 35% reduction in asset weight compared to standard AVIF and 68% compared to JPEG, WPG directly accelerates the Largest Contentful Paint metric, particularly on high-latency mobile networks.

### 11.3 Crawler Indexing & Fallbacks
- **Googlebot & Chromium Crawlers:** Modern search engines render custom elements and execute JavaScript, indexing `<wpg-img alt="...">` natively.
- **No-JS Scrapers & Social Previews:** For social media bots (WhatsApp, Facebook, Twitter) that only parse `<head>` tags without JS execution, maintain standard OpenGraph tags (`<meta property="og:image" content="...">`) alongside a semantic `<noscript>` tag:
```html
<wpg-img src="/images/hero.wpg" alt="Product Hero">
  <noscript>
    <img src="/images/hero.jpg" alt="Product Hero" width="800" height="600" loading="lazy">
  </noscript>
</wpg-img>
```

### 11.4 Production Case Study (any2prompt.com: 96/100 Mobile PageSpeed)
The real-world efficacy of WPG was verified in production on an active AI prompt engineering web platform ([any2prompt.com](https://any2prompt.com)):

| Metric / Asset Group | Before (Legacy JPG/PNG) | With WPG & `<wpg-img>` | Real-World Impact |
|---|:---:|:---:|:---:|
| **Google PageSpeed Mobile Score** | **74 - 75** (Orange) | **96 / 100** (Emerald) 🚀 | **+21 Points jump into top 1–2% globally** |
| **Hero Feature Visual** | 4.19 MB | **87.9 KB** | **-97.9%** (47.7x reduction) |
| **Interactive Gallery Assets** | 8.69 MB (10 photos) | **993 KB** | **-88.6%** (~7.7 MB bandwidth saved) |
| **Cumulative Layout Shift (CLS)** | 0.01 | **0.00** | **0.000 (Flawless Zero-CLS)** |
| **First Contentful Paint (FCP)** | 0.8 s | **0.7 s** | **0.7s sub-second paint** |
| **Largest Contentful Paint (LCP)** | 3.1 s (Red) | **1.8 s** (Green) | **-42% faster contentful paint** |

### 11.5 Performance Architecture: Lazy Loading & Script Defer
To achieve consistent 95–100/100 scores in mobile Lighthouse testing:
1. **Script Deferral:** Always load the WPG decoder bundle non-blockingly via `defer`:
   ```html
   <script src="/js/wpg-bundle.js" defer></script>
   ```
2. **Native Lazy Loading:** Always specify `loading="lazy"` on below-the-fold assets:
   ```html
   <wpg-img src="/images/gallery/portrait.wpg" alt="Portrait" width="300" height="300" loading="lazy"></wpg-img>
   ```
   The `<wpg-img>` component automatically attaches an internal `IntersectionObserver` with a `200px` root margin to defer network requests until the image nears the viewport.
3. **Avoid Inline `background-image` in JS Templates:** When rendering dynamic templates via JavaScript `innerHTML`, do not include redundant `<noscript><div style="background-image:url('legacy.jpg')"></div></noscript>` blocks, as browser CSS engines speculatively initiate background image downloads for inline style declarations.

---

## 12. Local Testing Playground (`playground/`)

A dedicated sandbox environment is available at `playground/`:
- `playground/input/`: Drop area for arbitrary test images (`.jpg`, `.png`, `.webp`).
- `playground/output/`: Destination directory for encoded `.wpg` files and decoded PNG verifications.
- `playground/test-encode.ps1`: Automated 1-click PowerShell runner that encodes, decodes, extracts micro-previews, prints compression savings tables, and runs bitstream benchmarks.
- `playground/index.html`: Standalone glassmorphic GUI with real-time file inspection, drag-and-drop decoding, and header metadata analysis.
- `playground/wpg-bundle.js`: Self-contained bundle with embedded base64 WebAssembly for 100% offline testing under the `file:///` protocol without CORS blocks.


