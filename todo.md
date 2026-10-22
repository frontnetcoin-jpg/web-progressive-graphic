# 📋 WPG (Web Progressive Graphic) — Master Project Todo List

---

## 🟢 1. Όσα έχουν ήδη υλοποιηθεί (Completed Milestones)

### 1.1 Core Engine & Container Architecture
- [x] **Ultra-Lean Binary Container (16-byte fixed header):** Εξάλειψη του περιττού βάρους 400–800B των κουτιών ISOBMFF του AVIF.
- [x] **16-Byte Instant Micro-Preview:** Αυτόματη εξαγωγή 4 αντιπροσωπευτικών χρωμάτων K-means και 4x4 spatial grid για **0 Cumulative Layout Shift (CLS)** σε < 0.05ms.
- [x] **Parametric Film Grain Synthesis:**
  - Ανίχνευση και μέτρηση διακύμανσης θορύβου αισθητήρα κάμερας μέσω τελεστή Laplace.
  - Pre-denoising φίλτρο πριν την κωδικοποίηση AV1 (εξοικονόμηση bits σε υψηλές συχνότητες DCT).
  - Ντετερμινιστική σύνθεση φυσικού κόκκου φιλμ (Xorshift32 PRNG) κατά το decode (**+20% έως +35% ανώτερη συμπίεση**).
- [x] **Adaptive Tri-Mode Engine:**
  - **Mode 0 (AV1 Enhanced Intra):** Lossy & high-fidelity φωτογραφική συμπίεση με βαθμονομημένη καμπύλη κβαντιστή.
  - **Mode 1 (Palette Lossless):** Έως 256 χρώματα + 2D Run-Length Encoding + Zstandard (ιδανικό για UI, logos, screenshots).
  - **Mode 2 (Modular Reversible Lossless):** Αντιστρέψιμος χρωματικός μετασχηματισμός YCoCg-R + Paeth/Median predictors + Zstd.
- [x] **Bitstream-Level AVIF Decoding & 100% Metric Accuracy:**
  - Απομόνωση του color AV1 OBU μέσα από το κουτί `mdat` παρακάμπτοντας το monochrome alpha stream.
  - Ανεξάρτητος υπολογισμός PSNR & SSIM pixel-προς-pixel για AVIF, WPG, JPEG, WebP και PNG.
- [x] **Lossy WebP Engine (Google `libwebp-sys`):** Πλήρης ενσωμάτωση του `webp` crate σε CLI benchmarks και Web server.

---

### 1.2 Native CLI & Web Showcase GUI
- [x] **Ενοποιημένο CLI (`wpg`):**
  - `wpg encode <in> <out> [-q] [-s]`
  - `wpg decode <in> <out> [--preview-only]`
  - `wpg info <in>`
  - `wpg bench <in> [-q]` (συγκριτικό table με PSNR, SSIM, bpp)
  - `wpg batch <in_dir> [-o <out_dir>] [-q] [-s] [-r]` (μαζική μετατροπή φακέλων)
  - `wpg serve [--port <port>]`
- [x] **Glassmorphic Web Showcase GUI (Port 3000):**
  - Interactive split comparison slider με συγχρονισμό 1:1.
  - Zoom magnifier lens & διόρθωση EXIF orientation.
  - Real-time sliders για Quality, Speed, Subsampling, Film Grain toggle.
  - Drag & Drop δικών σου εικόνων.
  - Κουμπιά άμεσης λήψης (`.wpg`, `.png`, `.bin`).

---

### 1.3 WebAssembly & Client-Side Integration
- [x] **Wasm Sub-Crate (`crates/wpg-wasm`):** Μεταγλώττιση σε `wasm32-unknown-unknown` χωρίς POSIX/libc dependencies.
- [x] **Wasm Binary (`wpg-decoder.wasm`):** Μέγεθος μόλις **61 KB** (~18 KB gzipped).
- [x] **Drop-in Web Component (`<wpg-img>` / `wpg-element.js`):**
  - 100% Offline client-side decoding στον browser χωρίς server roundtrip.
  - Αστραπιαίο 16B preview (< 0.05ms) με ομαλό CSS blur & crossfade.
  - Real-time σύνθεση κόκκου φιλμ απ' ευθείας στα canvas pixels.
  - Αυτόματο fallback σε server decode αν ο browser δεν υποστηρίζει Wasm/AVIF.
- [x] **Server Endpoints:** `/wpg-decoder.wasm`, `/wpg-element.js`, `/api/encode`, `/api/decode`, `/api/sample.wpg`.

---

### 1.4 Distribution, Packaging & Documentation
- [x] **NPM Package Structure (`packages/wpg-img/`):**
  - `package.json` έτοιμο για δημοσίευση.
  - ESM module (`dist/index.js`), TypeScript typings (`dist/index.d.ts`), Wasm binary.
  - Programmatic API `decodeWpg(arrayBuffer)`.
  - Πλήρες `packages/wpg-img/README.md` με παραδείγματα για React, Next.js, Vue, Nuxt, Svelte, Vite.
- [x] **Standalone Binary Installers:**
  - `scripts/install.ps1` (Windows PowerShell αυτόματη εγκατάσταση στο PATH).
  - `scripts/install.sh` (Linux / macOS Bash installer).
  - `scripts/package-release.ps1` (δημιουργία `wpg-v1.0.0-windows-x86_64.zip`).
- [x] **GitHub Actions Workflow:** `.github/workflows/release.yml` για automated cross-platform builds & npm publishing με κάθε release tag.
- [x] **Τεκμηρίωση & AI Agent Skills:**
  - `README.md`: Οδηγός χρήσης, benchmarks, CLI, web component.
  - `DOCUMENTATION.md`: 10 αναλυτικές τεχνικές ενότητες format architecture.
  - `SPECIFICATION.md`: Binary format specification v1.0.
  - Antigravity Agent Skill: Σε `.agents/skills/wpg/SKILL.md` και παγκόσμια στο `C:\Users\user\.gemini\config\skills\wpg\SKILL.md`.

---

## 🚀 2. Όσα πρέπει να κάνουμε μαζί (Upcoming Actions)

### 📌 Βήμα 1: Δοκιμή δικής σου φωτογραφίας (`me.wpg`)
- [ ] Επιλογή μιας προσωπικής σου φωτογραφίας (π.χ. `me.jpg`).
- [ ] Μετατροπή της σε WPG μέσω CLI:
  ```powershell
  .\target\release\wpg.exe encode me.jpg me.wpg --quality 80
  ```
- [ ] Δοκιμή σε πραγματικό περιβάλλον ιστοσελίδας με το tag:
  ```html
  <script type="module" src="/wpg-element.js"></script>
  <wpg-img src="me.wpg" alt="My Profile"></wpg-img>
  ```

---

### 📌 Βήμα 2: Git Commit & Σύνδεση με GitHub
- [ ] Έλεγχος των αρχείων προς commit (`git status`).
- [ ] Αρχικό commit ολόκληρου του project:
  ```powershell
  git add .
  git commit -m "feat: initial release of WPG image format v1.0.0"
  ```
- [ ] Δημιουργία public repository στο GitHub προφίλ σου (π.χ. `github.com/<username>/wpg`).
- [ ] Σύνδεση remote και push:
  ```powershell
  git remote add origin https://github.com/<username>/wpg.git
  git branch -M master
  git push -u origin master
  ```
- [ ] Δημιουργία και push του πρώτου release tag:
  ```powershell
  git tag v1.0.0
  git push origin v1.0.0
  ```

---

### 📌 Βήμα 3: Επίσημο Δημόσιο Publish στο NPM (npmjs.com)
- [ ] Δημιουργία δωρεάν λογαριασμού στο [npmjs.com](https://www.npmjs.com/) (αν δεν υπάρχει ήδη).
- [ ] Login από το τερματικό:
  ```powershell
  npm login
  ```
- [ ] Δημοσίευση του πακέτου:
  ```powershell
  cd packages\wpg-img
  npm publish --access public
  ```
- [ ] Επαλήθευση ότι το πακέτο εγκαθίσταται παγκοσμίως:
  ```bash
  npm install wpg-img
  ```

---

### 📌 Βήμα 4: Ρύθμιση Αυτοματισμού στο GitHub (CI/CD)
- [ ] Δημιουργία `Access Token` στο npmjs.com (`Automation` type).
- [ ] Προσθήκη του token στα **Repository Secrets** του GitHub με όνομα `NPM_TOKEN`.
- [ ] Επαλήθευση ότι με κάθε μελλοντικό tag (π.χ. `v1.0.1`), το GitHub Actions θα κάνει αυτόματα compile τα binaries (Windows, Linux, macOS) και θα ανεβάζει στο npmjs.com.

---

### 📌 Βήμα 5: Μελλοντικές Επεκτάσεις (Προαιρετικά)
- [ ] **WordPress Plugin:** Αυτόματη μετατροπή όλων των uploads στη Media Library σε `.wpg`.
- [ ] **Laravel Middleware / Package:** Πακέτο για seamless rendering του `<wpg-img>` σε Blade templates.
- [ ] **Service Worker Interceptor:** Service worker που μετατρέπει τα requests για `*.wpg` on-the-fly για κλασικά `<img>` tags χωρίς αλλαγή του HTML markup.
