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

# Πλήρες Benchmark (WPG vs AVIF vs PNG vs JPEG vs WebP) με PSNR & SSIM
.\target\release\wpg.exe bench input.png --quality 80

# Native Desktop Viewer (Υπερταχύ Win32 GDI με Pan & Zoom)
.\target\release\wpg.exe view output.wpg
# ή απευθείας:
.\target\release\wpg-view.exe output.wpg

# Εγγραφή στο Windows Explorer (1-Click, Χωρίς Δικαιώματα Διαχειριστή)
# Ενεργοποιεί: Διπλό κλικ, Context Menu & Live Explorer Thumbnails (wpg_thumb.dll)
.\target\release\wpg.exe register

# Κατάργηση εγγραφής από το Windows Explorer:
.\target\release\wpg.exe unregister
```

### Windows Shell Integration & Thumbnail Provider (`wpg_thumb.dll`)
* **Live Thumbnails στο Windows Explorer:** Υλοποίηση του Windows Shell COM interface `IThumbnailProvider` και `IInitializeWithFile` σε Pure Rust (`crates/wpg-thumb`). Αποκωδικοποιεί αστραπιαία το AV1 bitstream και επιστρέφει hardware-ready 32-bit ARGB DIB bitmaps σε οποιοδήποτε μέγεθος (Extra Large, Large, Medium, Small Icons, και Preview Pane).
* **Native Desktop Viewer (`wpg-view.exe`):** Αυτόνομο Win32 binary με double-buffered GDI canvas, smooth mouse-wheel zoom, drag-to-pan, directory browsing με βελάκια, PNG export με `S`, fullscreen με `F`/`F11`, overlay HUD με `I`, και drag & drop αρχείων.
* **Explorer Context Menu:** Δεξί κλικ σε οποιοδήποτε `.wpg` εμφανίζει: "Open with WPG Viewer", "Convert to PNG" και "WPG Image Analysis".
* **Zero-Admin Registration:** Όλα τα COM CLSID και ProgID κλειδιά καταχωρούνται στο `HKCU:\Software\Classes` χωρίς να ζητείται UAC / Administrator prompt.

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

## 4. Δημόσιο Οικοσύστημα & Διανομή (NPM & GitHub)

* **GitHub Repository:** [https://github.com/frontnetcoin-jpg/web-progressive-graphic](https://github.com/frontnetcoin-jpg/web-progressive-graphic) (Public, Branch `main`)
* **NPM Registry Package:** [https://www.npmjs.com/package/wpg-img](https://www.npmjs.com/package/wpg-img) (`wpg-img@1.0.0`)
  - Εγκατάσταση: `npm install wpg-img`
  - Περιέχει: TypeScript typings (`dist/index.d.ts`), ESM bundle (`dist/index.js`), 61 KB Wasm (`wpg-decoder.wasm`).

### Standalone Binary Installers & Release Script
- **Windows PowerShell:** `scripts/install.ps1`
- **Linux/macOS Bash:** `scripts/install.sh`
- **Release Packaging:** `scripts/package-release.ps1` (δημιουργεί το `wpg-v1.0.0-windows-x86_64.zip`)

---

## 5. Τοπικό Περιβάλλον Δοκιμών (`playground/`)

Για άμεσες δοκιμές οποιασδήποτε εικόνας:
- `playground/input/`: Τοποθέτηση εικόνων (`.jpg`, `.png`, `.webp`).
- `playground/output/`: Παραγόμενα `.wpg` και decoded αρχεία.
- `playground/test-encode.ps1`: 1-Click script συμπίεσης, υπολογισμού οικονομίας και αυτόματων benchmarks.
- `playground/index.html`: Web GUI για ζωντανή δοκιμή με drag-and-drop και ανάλυση header.
- `playground/wpg-bundle.js`: Αυτόνομο bundle με ενσωματωμένο base64 WebAssembly για 100% ασφαλή λειτουργία ακόμα και με πρωτόκολλο `file:///`.

---

## 6. SEO & Core Web Vitals Strategy

- **CLS Score 0.000:** Το 16-byte fixed container και το 16-byte micro-preview δεσμεύουν τις διαστάσεις της εικόνας σε `< 0.05 ms`, προσφέροντας άριστο Google ranking score.
- **LCP Optimization:** 20% έως 35% ταχύτερο LCP συγκριτικά με το AVIF, και έως 68% μικρότερο βάρος σε σχέση με το JPEG.
- **Production Milestone (any2prompt.com):** Επιβεβαιώθηκε στην πράξη σε ζωντανό SaaS application εκτοξεύοντας το Google PageSpeed score από **74-75** σε **96/100 (Καταπράσινο)**!
  - Gallery (10 εικόνες): Από **8.69 MB** σε **993 KB** (**-88.6%**).
  - Hero visual: Από **4.19 MB** σε **87.9 KB** (**-97.9%**).
- **Αρχιτεκτονικοί Κανόνες Ενσωμάτωσης (Web Best Practices):**
  1. **Lazy Loading:** Χρήση `loading="lazy"` στα `<wpg-img>` για below-the-fold assets. Το component ενεργοποιεί εσωτερικό `IntersectionObserver` με `200px` rootMargin αποτρέποντας το μπούκωμα του δικτύου.
  2. **Script Deferral:** Φόρτωση του bundle script πάντα με `defer` (`<script src="/js/wpg-bundle.js" defer></script>`).
  3. **No Duplicate Fetch:** Έλεγχος `this._loadedUrl` για αποτροπή διπλών HTTP αιτημάτων.
  4. **No-JS Crawler Fallback:**
     ```html
     <wpg-img src="/images/hero.wpg" alt="Hero Title" loading="lazy">
       <noscript>
         <img src="/images/hero.jpg" alt="Hero Title" width="800" height="600" loading="lazy">
       </noscript>
     </wpg-img>
     ```

---

## 7. Κανόνες Ανάπτυξης & Safety Rules

1. **Windows Binary Lock (`os error 32`):**
   - Πριν τρέξετε `cargo build --release`, **τερματίστε** πάντα τυχόν ανοιχτό process του server (`taskkill /F /IM wpg.exe` ή `manage_task kill`), αλλιώς τα Windows κλειδώνουν το εκτελέσιμο αρχείο στο δίσκο.
2. **Ανεξαρτησία Μετρήσεων Benchmarks (100% Confidence Rule):**
   - Μην χρησιμοποιείτε ποτέ placeholders για metrics τρίτων formats.
   - Το AVIF αποκωδικοποιείται bitstream-level παρακάμπτοντας το monochrome alpha payload στο κουτί `mdat`.
   - Το WebP υποστηρίζει **Lossy WebP (Google libwebp-sys)** και **Lossless WebP (VP8L)** με πραγματικό υπολογισμό PSNR και SSIM pixel-προς-pixel.
3. **Wasm Compatibility:**
   - Το `crates/wpg-wasm` διατηρείται εξαιρετικά ελαφρύ (< 65 KB) χωρίς εξαρτήσεις POSIX/libc ώστε να μεταγλωττίζεται αυτόνομα σε `wasm32-unknown-unknown`.
4. **Master Memory:**
   - Συμβουλευτείτε πάντα το [`MEMORY.md`](file:///c:/Users/user/Desktop/wpg/MEMORY.md) για πλήρεις τεχνικές προδιαγραφές, εξισώσεις και ιστορικό αποφάσεων.
5. **Ιστορική Διασάφηση (WordPerfect Disambiguation):**
   - Το WPG (Web Progressive Graphic) είναι σύγχρονο web standard του 2026 και δεν έχει καμία σχέση με το παρωχημένο format των '80s *WordPerfect Graphics* (`.wpg`). Όπως το TypeScript (`.ts`) έναντι του MPEG Transport Stream (`.ts`), η κατάληξη διακρίνεται από το context, το MIME type (`image/wpg`) και τα binary magic bytes `b"WPG\x01"`.

