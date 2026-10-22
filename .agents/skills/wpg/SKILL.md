---
name: wpg
description: >-
  Comprehensive engineering, workflow, and testing skill for WPG (Web Progressive Graphic),
  the next-generation AV1-based image format in pure Rust. Use when building, testing,
  benchmarking, encoding/decoding, running batch conversions, managing the Glassmorphic
  Web Showcase GUI or REST API, tuning film grain synthesis, working with the 16-byte zero-CLS
  micro-preview, or integrating the <wpg-img> web component.
---

# ⚡ Skill: WPG (Web Progressive Graphic) Next-Gen Image Format

Αυτό το skill παρέχει πλήρη τεχνική τεκμηρίωση, αρχιτεκτονική ανάλυση, workflows, εντολές CLI και μεθοδολογία ανάπτυξης και συντήρησης για το format εικόνας **WPG (.wpg)**.

---

## 1. Επισκόπηση & Αρχιτεκτονική WPG

Το WPG είναι ένα format εικόνας επόμενης γενιάς σε **Pure Rust**, βασισμένο στον αλγόριθμο **AV1 Intra**, απαλλαγμένο από το container bloat του **.avif (ISOBMFF)** και βελτιστοποιημένο για το σύγχρονο Web:

```
[Εικόνα Εισόδου (PNG, JPEG, WebP, Raw RGBA)]
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
  │  • Primary Payload (OBU Bitstream / Zstd)    │
  └──────────────────────┬───────────────────────┘
                         │
                         ▼
             [Τελικό Αρχείο *.wpg]
```

### Βασικά Modules του Codebase (`src/`)

* [`src/header.rs`](file:///c:/Users/user/Desktop/wpg/src/header.rs): Διαχείριση του 16-byte binary header, bitflags (`FLAG_MICRO_PREVIEW`, `FLAG_FILM_GRAIN`, `FLAG_LOSSLESS`), σειριοποίηση και επικύρωση.
* [`src/preview.rs`](file:///c:/Users/user/Desktop/wpg/src/preview.rs): Δημιουργία και αποκωδικοποίηση του **16-byte micro-preview** (4 χρώματα RGB K-means + 16 2-bit spatial indices σε 4x4 grid).
* [`src/grain.rs`](file:///c:/Users/user/Desktop/wpg/src/grain.rs): Εκτίμηση θορύβου με τελεστή Laplace, απαλή εξομάλυνση πριν την κωδικοποίηση, και ντετερμινιστική σύνθεση κόκκου (Xorshift32 PRNG) κατά το decode.
* [`src/color.rs`](file:///c:/Users/user/Desktop/wpg/src/color.rs): Χρωματικοί μετασχηματισμοί sRGB $\leftrightarrow$ YUV (4:2:0, 4:2:2, 4:4:4) κατά BT.709 & BT.601, καθώς και μαθηματικά αντιστρέψιμος YCoCg-R.
* [`src/lossless.rs`](file:///c:/Users/user/Desktop/wpg/src/lossless.rs): Mode 1 (Palette $\le 256$ χρωμάτων + 2D RLE + Zstd) και Mode 2 (YCoCg-R + Paeth/Median predictors + Zstd).
* [`src/av1_codec.rs`](file:///c:/Users/user/Desktop/wpg/src/av1_codec.rs): Pure Rust AV1 intra encoder (`rav1e`) και decoder (`rav1d`). Περιέχει βαθμονομημένη καμπύλη κβαντιστή και τη συνάρτηση `decode_avif_to_rgba` για bitstream-level AVIF decode.
* [`src/encoder.rs`](file:///c:/Users/user/Desktop/wpg/src/encoder.rs) & [`src/decoder.rs`](file:///c:/Users/user/Desktop/wpg/src/decoder.rs): High-level pipelines κωδικοποίησης και αποκωδικοποίησης WPG.
* [`src/metrics.rs`](file:///c:/Users/user/Desktop/wpg/src/metrics.rs): Ακριβής υπολογισμός PSNR, SSIM, bpp και ποσοστών εξοικονόμησης pixel-προς-pixel.
* [`src/server.rs`](file:///c:/Users/user/Desktop/wpg/src/server.rs): Ενσωματωμένος HTTP server (`tiny_http`) με Glassmorphic Web Showcase GUI, zoom lens, REST API και serving του Web Component.
* [`src/bin/wpg.rs`](file:///c:/Users/user/Desktop/wpg/src/bin/wpg.rs): Ενοποιημένο CLI εργαλείο (`encode`, `decode`, `batch`, `bench`, `info`, `serve`).
* [`wpg-element.js`](file:///c:/Users/user/Desktop/wpg/wpg-element.js): Native Web Component `<wpg-img>` για zero-CLS placeholders και progressive rendering στο browser.

---

## 2. Οδηγός Χρήσης CLI (`wpg`)

### Εκκίνηση του Web Showcase & Viewer
```powershell
.\target\release\wpg.exe serve --port 3000
```
- URL: `http://127.0.0.1:3000`
- Παρέχει σύγκριση split slider 1:1, zoom magnifier, EXIF orientation διόρθωση, ζωντανά ρυθμιστικά και κουμπιά download `.wpg`, `.png`, και `.bin`.

### Μαζική Μετατροπή Φακέλων (Batch Conversion)
```powershell
# Μετατροπή όλων των εικόνων σε έναν φάκελο με ποιότητα 75:
.\target\release\wpg.exe batch C:\path\to\images -q 75

# Αναδρομική μετατροπή υποκαταλόγων και αποθήκευση σε νέο φάκελο:
.\target\release\wpg.exe batch .\input_photos -o .\output_wpg -q 80 -s 6 -r
```

### Μεμονωμένες Εντολές CLI
```powershell
# Κωδικοποίηση (Encode)
.\target\release\wpg.exe encode input.png output.wpg --quality 80 --speed 6

# Αποκωδικοποίηση (Decode σε PNG)
.\target\release\wpg.exe decode output.wpg restored.png

# Εξαγωγή μόνο του 16-byte micro-preview (4x4 PNG)
.\target\release\wpg.exe decode output.wpg placeholder.png --preview-only

# Πληροφορίες Header & Container (Info)
.\target\release\wpg.exe info output.wpg

# Πλήρες Benchmark (WPG vs PNG vs JPEG vs WebP) με PSNR & SSIM
.\target\release\wpg.exe bench input.png --quality 80
```

---

## 3. Web Component `<wpg-img>` Workflow

Για ενσωμάτωση του WPG σε οποιαδήποτε web εφαρμογή (HTML/React/Vue/Laravel):

```html
<!-- 1. Φόρτωση του script -->
<script src="http://localhost:3000/wpg-element.js"></script>

<!-- 2. Χρήση του tag -->
<wpg-img 
    src="http://localhost:3000/api/sample.wpg" 
    alt="High-Efficiency WPG Asset" 
    style="width: 100%; max-width: 800px; border-radius: 12px;">
</wpg-img>
```

### Μηχανισμός Προοδευτικής & Offline Wasm Φόρτωσης:
1. Το component διαβάζει τα **πρώτα 32 bytes** του αρχείου μέσω HTTP Range request ή stream slice.
2. Αντλεί το **16-byte micro-preview** και το σχεδιάζει άμεσα σε `<canvas width="4" height="4">` με `filter: blur(14px)`.
3. **Αποτέλεσμα:** Ο χρήστης βλέπει αμέσως τα χρώματα και τη διάταξη της εικόνας σε **< 0.05 ms**, εξαλείφοντας εντελώς το **Cumulative Layout Shift (CLS)**.
4. **100% Offline Wasm Decoding:** Το Wasm module (`wpg-decoder.wasm`, μόλις 61 KB) διαβάζει το raw AV1 OBU stream, συνθέτει ακαριαία (0.005 ms) το ελαφρύ ISOBMFF header στη μνήμη του browser και καλεί την ενσωματωμένη GPU video/image engine του περιηγητή (`ImageBitmap`).
5. **Real-Time Client Grain:** Αν η εικόνα έχει κόκκο (`FLAG_FILM_GRAIN`), το Wasm εφαρμόζει τη ντετερμινιστική σύνθεση Xorshift32 PRNG απ' ευθείας στα pixels του canvas.

---

## 4. Διανομή (NPM & Standalone Binaries)

### NPM Package (`wpg-img`)
- Βρίσκεται στο φάκελο `packages/wpg-img/` με πλήρη TypeScript bindings (`index.d.ts`), ESM και CommonJS bundles.
- Χρήση: `npm install wpg-img`.

### Standalone Binary Installers & Release Script
- **Windows PowerShell:** `scripts/install.ps1`
- **Linux/macOS Bash:** `scripts/install.sh`
- **Packaging:** `scripts/package-release.ps1` (δημιουργεί το `wpg-v1.0.0-windows-x86_64.zip`)

---

## 5. Κανόνες Ανάπτυξης & Safety Rules

1. **Windows Binary Lock (`os error 32`):**
   - Πριν τρέξετε `cargo build --release`, **τερματίστε** πάντα τυχόν ανοιχτό process του server (`taskkill /F /IM wpg.exe` ή `manage_task kill`), αλλιώς τα Windows κλειδώνουν το εκτελέσιμο αρχείο στο δίσκο.
2. **Ανεξαρτησία Μετρήσεων Benchmarks (100% Confidence Rule):**
   - Μην χρησιμοποιείτε ποτέ placeholders για metrics τρίτων formats.
   - Το AVIF αποκωδικοποιείται bitstream-level παρακάμπτοντας το monochrome alpha payload στο κουτί `mdat`.
   - Το WebP υποστηρίζει πλέον **Lossy WebP (Google libwebp-sys)** και **Lossless WebP (VP8L)** με πραγματικό υπολογισμό PSNR και SSIM pixel-προς-pixel.
3. **Wasm Compatibility:**
   - Το `crates/wpg-wasm` διατηρείται εξαιρετικά ελαφρύ (< 65 KB) χωρίς εξαρτήσεις POSIX/libc ώστε να μεταγλωττίζεται αυτόνομα σε `wasm32-unknown-unknown`.
