# 🧪 WPG Local Testing Playground

Καλώς ήρθες στον τοπικό φάκελο δοκιμών του **WPG (Web Progressive Graphic)**!

Εδώ μπορείς να δοκιμάσεις εύκολα και γρήγορα τις δικές σου εικόνες (φωτογραφίες, λογότυπα, screenshots), να δεις πόσα bytes εξοικονομείς και να τις δεις ζωντανά στον browser με το Web Component `<wpg-img>`.

---

## 📁 Δομή του Playground

```
playground/
├── input/             # 📥 Ρίξε εδώ όποιες εικόνες θες (.jpg, .png, .webp)
├── output/            # 📤 Εδώ αποθηκεύονται τα παραγόμενα .wpg και decoded .png
├── test-encode.ps1    # ⚡ Script 1-click για αυτόματη συμπίεση & benchmarks
├── index.html         # 🌐 Standalone web page για δοκιμή του <wpg-img>
└── README.md          # 📖 Αυτός ο οδηγός
```

---

## 🚀 Πώς να κάνεις δοκιμές σε 3 απλά βήματα:

### Βήμα 1: Ρίξε τις εικόνες σου
Αντίγραψε οποιαδήποτε εικόνα σου (π.χ. `photo.jpg` ή `logo.png`) μέσα στον φάκελο **`playground/input/`**.

### Βήμα 2: Τρέξε τη δοκιμή (1-Click)
Άνοιξε το PowerShell μέσα στον φάκελο `playground` και τρέξε:
```powershell
.\test-encode.ps1
```
Το script θα:
1. Συμπιέσει όλες τις εικόνες του `input/` σε `.wpg` μέσα στο `output/`.
2. Υπολογίσει αυτόματα πόσα **KB και % εξοικονόμηση** πέτυχες σε σχέση με το αρχικό αρχείο.
3. Εξάγει το **16-byte micro-preview** για να δεις το instant placeholder.
4. Τρέξει αυτόματο **Benchmark** (PSNR, SSIM, Bits Per Pixel).

---

### Βήμα 3: Δες το αποτέλεσμα στον Browser
Μπορείς να ανοίξεις το `index.html`:
* Είτε ανοίγοντας απευθείας το `playground/index.html` στον browser σου (ή με Live Server).
* Είτε μέσω του ενσωματωμένου showcase server:
  ```powershell
  # Από τον κεντρικό φάκελο wpg:
  .\target\release\wpg.exe serve --port 3000
  ```
  Και επισκέψου το **[http://localhost:3000](http://localhost:3000)** όπου μπορείς να σύρεις (drag & drop) απευθείας την εικόνα σου!

---

## 🛠️ Χρήσιμες Μεμονωμένες Εντολές CLI:

```powershell
# 1. Συμπίεση μίας εικόνας με συγκεκριμένο Quality (1..100):
..\target\release\wpg.exe encode input\photo.jpg output\photo.wpg --quality 80

# 2. Αποσυμπίεση του .wpg πίσω σε .png:
..\target\release\wpg.exe decode output\photo.wpg output\restored.png

# 3. Εξαγωγή μόνο του 16-byte Micro-Preview (blur placeholder):
..\target\release\wpg.exe decode output\photo.wpg output\preview.png --preview-only

# 4. Πληροφορίες για το container & τα flags του αρχείου:
..\target\release\wpg.exe info output\photo.wpg

# 5. Πλήρες συγκριτικό Benchmark (WPG vs WebP vs AVIF vs PNG vs JPEG):
..\target\release\wpg.exe bench input\photo.jpg --quality 80
```
