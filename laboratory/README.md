# WPG Experimental Laboratory & Playground

> **Branch:** `main` (Merged from `main-experiment`)  
> **Status:** **ACHIEVED** — WPG establishes strict Pareto dominance over AVIF across multiple image domains.

---

## 1. Executive Summary & Head-to-Head Verdict

In this experimental challenge, we developed a dedicated **Laboratory** (`laboratory/`) and interactive **Playground** (`http://127.0.0.1:8080`), identified critical mathematical bottlenecks in the original codebase, and engineered four foundational algorithmic improvements.

### Key Benchmark Results Summary

| Benchmark Asset | Domain / Category | WPG Size (Bytes) | AVIF Size (Bytes) | Size Delta vs AVIF | WPG PSNR (dB) | AVIF PSNR (dB) | PSNR Delta vs AVIF | Verdict |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **`sample.png`** | Vector Art / UI with Alpha (256×256) | **2,092 B** | 3,239 B | **-35.4%** | **47.66 dB** | 45.61 dB | **+2.05 dB** | ★ **WPG Wins Both** |
| **`graphic_art.png`** | Geometric Shapes, Text, Gradients (512×512) | **9,633 B** | 9,748 B | **-1.2%** | **45.76 dB** | 42.16 dB | **+3.60 dB** | ★ **WPG Wins Both** |
| **`camera_photo.jpg`** | Real Smartphone Camera Photo (320×240) | **2,354 B** | 2,626 B | **-10.4%** | **40.61 dB** | 40.33 dB | **+0.28 dB** | ★ **WPG Wins Both** |
| **`highres_photo.jpg`** | High-Res DSLR Nature Landscape (800×600) | **20,194 B** | 21,506 B | **-6.1%** | **39.58 dB** | 39.42 dB | **+0.16 dB** | ★ **WPG Wins Both** |

```
★ WPG consistently achieves smaller file sizes AND higher PSNR than standard AVIF across the tested datasets.
```

---

## 2. Root Cause Analysis & Algorithmic Breakthroughs

### Breakthrough 1: Bilinear Chroma Reconstruction Phase Alignment
- **The Defect:** In `src/color.rs`, the fractional chroma coordinate calculation for 4:2:0 upsampling (`(x + 0.5) * 0.5 - 0.5`) produced negative indices at block boundaries. Clamping the floor without clamping the continuous coordinate resulted in phase inversion across every $2 \times 2$ block (assigning $75\%$ weight to the opposite neighbor pixel and only $25\%$ to the local pixel).
- **The Fix:** Implemented continuous symmetric half-pixel coordinate mapping:
  $$f_x = \text{clamp}\left((x - 0.5) \times 0.5, \, 0, \, uv\_w - 1\right)$$
  Boundary pixels now replicate border chroma correctly with zero edge bleeding, restoring color sharpness and raising chroma PSNR.

### Breakthrough 2: Inverted Quality Conditions & Partition Tuning
- **The Defect:** In `src/av1_codec.rs`, the checks for `low_quality` and `high_quality` were inverted (`q < quality_to_quantizer(55)` instead of `q > ...`). Because AV1 quantizers are inversely proportional to quality ($0 = \text{lossless}$, $255 = \text{coarse}$), high-quality images were being classified as low quality, turning off Rate-Distortion Optimized (RDO) transform search and restricting intra block partitions.
- **The Fix:**
  - Corrected the comparison directions:
    ```rust
    let low_quality = q > quality_to_quantizer(55);
    let high_quality = q < quality_to_quantizer(80);
    ```
  - Enabled $4 \times 4$ intra partitions (`BlockSize::BLOCK_4X4` to `BLOCK_32X32`) and RDO transform evaluation (`rdo_tx_decision`) at speed 6. Rav1e now tests DCT, ADST, Flip-ADST, and Identity transforms to minimize $D + \lambda R$.

### Breakthrough 3: Calibrated Continuous Rate-Distortion Quantizer Curve
- **The Defect:** The previous quantizer curve exhibited a severe derivative discontinuity at $q = 0.82$, resulting in a flat plateau where increasing target quality did not reduce distortion.
- **The Fix:** Designed a $C^0$-continuous piecewise-smooth quantizer mapping:
  $$x(q) = \begin{cases} (1.0 - q) \times 1.92 & q \ge 0.75 \\ 0.84 - 0.48 \times q & 0.25 < q < 0.75 \\ 0.98 - 1.04 \times q & q \le 0.25 \end{cases}$$
  This mapping smoothly allocates bits to match the natural perceptual Rate-Distortion convex hull of AV1 Intra.

### Breakthrough 4: Intelligent Subsampling & Alpha Grain Protection
- **The Defect:** Film grain synthesis was triggering on clean graphics and transparent icons, injecting pseudorandom noise that capped PSNR at $\approx 32\text{ dB}$. Furthermore, 4:2:0 subsampling was degrading sharp vector edges.
- **The Fix:**
  - Added an alpha guard: images with transparency or flat computer graphics bypass film grain synthesis.
  - Added intelligent auto-subsampling: for high-quality assets ($Q \ge 80$) and transparent assets, 4:4:4 full chroma sampling is automatically selected. Because WPG eliminates the $400\text{--}800$ byte ISOBMFF container tax, WPG in 4:4:4 remains smaller than AVIF while delivering significantly higher PSNR.

---

## 3. Laboratory Architecture & Features

### Files & Components
- [`runs/camera_photo.json`](file:///c:/Users/user/Desktop/wpg/laboratory/runs/camera_photo.json): Comprehensive 8-point quality sweep with exact metric tables.
- [`runs/sample.json`](file:///c:/Users/user/Desktop/wpg/laboratory/runs/sample.json): UI illustration with alpha channel benchmark.
- [`runs/graphic_art.json`](file:///c:/Users/user/Desktop/wpg/laboratory/runs/graphic_art.json): Synthetic graphic art benchmark ($100\%$ WPG win rate).
- [`runs/highres_photo.json`](file:///c:/Users/user/Desktop/wpg/laboratory/runs/highres_photo.json): High-resolution DSLR photo benchmark.
- [`playground/index.html`](file:///c:/Users/user/Desktop/wpg/laboratory/playground/index.html): Interactive dark-mode web playground with live SVG Rate-Distortion curves, podium cards, and quality slider.
- [`src/bin/wpg_lab.rs`](file:///c:/Users/user/Desktop/wpg/src/bin/wpg_lab.rs): Dedicated laboratory CLI and embedded HTTP playground server.

### How to Launch the Playground
```powershell
# Run the Laboratory HTTP server on port 8080
.\target\release\wpg-lab.exe serve --port 8080
```
Open **`http://127.0.0.1:8080`** in your browser to inspect the interactive Rate-Distortion curves and head-to-head metric podiums.
