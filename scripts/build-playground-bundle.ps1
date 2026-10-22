$ErrorActionPreference = "Stop"

$wasmPath = Join-Path $PSScriptRoot "..\wpg-decoder.wasm"
$wasmBytes = [System.IO.File]::ReadAllBytes($wasmPath)
$wasmB64 = [Convert]::ToBase64String($wasmBytes)

$jsTemplate = @'
/**
 * Standalone WPG Web Component & Decoder for Local Playground
 * Works 100% offline both on http:// and file:/// protocols!
 */
(function() {
    let wpgWasmModule = null;
    const EMBEDDED_WASM_B64 = "###WASM_B64###";

    function base64ToUint8(b64) {
        const bin = atob(b64);
        const len = bin.length;
        const bytes = new Uint8Array(len);
        for (let i = 0; i < len; i++) {
            bytes[i] = bin.charCodeAt(i);
        }
        return bytes;
    }

    async function getWpgWasm(wasmUrl) {
        if (wpgWasmModule) return wpgWasmModule;
        // Try network fetch if not on file://
        if (location.protocol !== 'file:' && wasmUrl) {
            try {
                const resp = await fetch(wasmUrl);
                if (resp.ok) {
                    const bytes = await resp.arrayBuffer();
                    const { instance } = await WebAssembly.instantiate(bytes, {});
                    wpgWasmModule = instance;
                    return wpgWasmModule;
                }
            } catch (e) {
                console.warn('Wasm fetch failed, using embedded fallback:', e);
            }
        }
        // Fallback to embedded base64 wasm (works 100% on file://)
        try {
            const bytes = base64ToUint8(EMBEDDED_WASM_B64);
            const { instance } = await WebAssembly.instantiate(bytes, {});
            wpgWasmModule = instance;
            return wpgWasmModule;
        } catch (e) {
            console.error('Failed to instantiate embedded WPG WebAssembly:', e);
            return null;
        }
    }

    class WpgImageElement extends HTMLElement {
        static get observedAttributes() { return ['src', 'alt', 'width', 'height', 'wasm-url']; }

        constructor() {
            super();
            this.attachShadow({ mode: 'open' });
            this.shadowRoot.innerHTML = `
                <style>
                    :host {
                        display: block;
                        position: relative;
                        overflow: hidden;
                        background: #0f172a;
                        border-radius: 8px;
                        line-height: 0;
                        width: 100%;
                        height: 100%;
                    }
                    canvas {
                        display: block;
                        width: 100%;
                        height: 100%;
                        object-fit: contain;
                        transition: filter 0.4s cubic-bezier(0.16, 1, 0.3, 1), transform 0.4s cubic-bezier(0.16, 1, 0.3, 1);
                    }
                    .preview-mode {
                        filter: blur(14px);
                        transform: scale(1.05);
                    }
                    .loaded-mode {
                        filter: blur(0px);
                        transform: scale(1.0);
                    }
                </style>
                <canvas id="cvs"></canvas>
            `;
        }

        connectedCallback() {
            if (this.getAttribute('src')) {
                this.loadWpg(this.getAttribute('src'));
            }
        }

        attributeChangedCallback(name, oldVal, newVal) {
            if (name === 'src' && oldVal !== newVal && newVal) {
                this.loadWpg(newVal);
            }
        }

        async loadWpg(url) {
            try {
                const resp = await fetch(url);
                if (!resp.ok) throw new Error('HTTP ' + resp.status);
                const buffer = await resp.arrayBuffer();
                await this.loadBytes(new Uint8Array(buffer));
            } catch (e) {
                console.warn('WPG fetch error:', e);
            }
        }

        async loadBytes(bytes) {
            const cvs = this.shadowRoot.getElementById('cvs');
            const ctx = cvs.getContext('2d');
            cvs.className = 'preview-mode';

            try {
                if (bytes.length < 16 || bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) {
                    throw new Error('Invalid WPG magic');
                }

                const w = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
                const h = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
                cvs.width = w;
                cvs.height = h;

                const flags = (bytes[12] << 8) | bytes[13];
                const hasPreview = (flags & 0x0002) !== 0;
                const hasGrain = (flags & 0x0004) !== 0;

                // 1. Instant 16B Micro-Preview (< 0.05ms)
                if (hasPreview && bytes.length >= 32) {
                    const p = bytes.subarray(16, 32);
                    const pal = [
                        [p[0], p[1], p[2]],
                        [p[3], p[4], p[5]],
                        [p[6], p[7], p[8]],
                        [p[9], p[10], p[11]]
                    ];
                    const imgData = ctx.createImageData(4, 4);
                    let bitIdx = 0;
                    for (let i = 0; i < 16; i++) {
                        const byteVal = p[12 + (bitIdx >> 3)];
                        const shift = 6 - (bitIdx & 7);
                        const cIdx = (byteVal >> shift) & 3;
                        bitIdx += 2;
                        imgData.data[i * 4] = pal[cIdx][0];
                        imgData.data[i * 4 + 1] = pal[cIdx][1];
                        imgData.data[i * 4 + 2] = pal[cIdx][2];
                        imgData.data[i * 4 + 3] = 255;
                    }
                    if (window.createImageBitmap) {
                        const bmp = await createImageBitmap(imgData);
                        ctx.imageSmoothingEnabled = true;
                        ctx.drawImage(bmp, 0, 0, w, h);
                    }
                }

                // 2. Decode via WebAssembly
                const wasm = await getWpgWasm(this.getAttribute('wasm-url') || '../wpg-decoder.wasm');
                if (wasm && wasm.exports) {
                    const mem = wasm.exports.memory;
                    const wpgPtr = wasm.exports.wpg_alloc(bytes.length);
                    new Uint8Array(mem.buffer).set(bytes, wpgPtr);

                    const outPtrPtr = wasm.exports.wpg_alloc(4);
                    const outLenPtr = wasm.exports.wpg_alloc(4);

                    const res = wasm.exports.wpg_wrap_to_avif(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
                    if (res === 0) {
                        const view = new DataView(mem.buffer);
                        const avifPtr = view.getUint32(outPtrPtr, true);
                        const avifLen = view.getUint32(outLenPtr, true);
                        const avifBytes = new Uint8Array(mem.buffer, avifPtr, avifLen).slice();
                        const blob = new Blob([avifBytes], { type: 'image/avif' });

                        if (window.createImageBitmap) {
                            try {
                                const bmp = await createImageBitmap(blob);
                                ctx.drawImage(bmp, 0, 0);
                                cvs.className = 'loaded-mode';

                                // Only apply client CPU grain on reasonable image sizes (< 4MP)
                                if (hasGrain && (w * h <= 4194304)) {
                                    try {
                                        let offset = 16 + (hasPreview ? 16 : 0);
                                        if (bytes.length >= offset + 8) {
                                            const grainScale = bytes[offset];
                                            const grainSeed = (bytes[offset + 1] << 8) | bytes[offset + 2];
                                            const imgData = ctx.getImageData(0, 0, w, h);
                                            const rgbaPtr = wasm.exports.wpg_alloc(imgData.data.length);
                                            new Uint8Array(wasm.exports.memory.buffer).set(imgData.data, rgbaPtr);
                                            wasm.exports.wpg_apply_film_grain(rgbaPtr, w, h, grainScale, grainSeed);
                                            imgData.data.set(new Uint8Array(wasm.exports.memory.buffer, rgbaPtr, imgData.data.length));
                                            ctx.putImageData(imgData, 0, 0);
                                            wasm.exports.wpg_free(rgbaPtr, imgData.data.length);
                                        }
                                    } catch (grainErr) {
                                        console.warn('Film grain skipped:', grainErr);
                                    }
                                }
                            } catch (bmpErr) {
                                console.error('createImageBitmap error:', bmpErr);
                            }
                        }
                        wasm.exports.wpg_free(avifPtr, avifLen);
                    }
                    wasm.exports.wpg_free(wpgPtr, bytes.length);
                    wasm.exports.wpg_free(outPtrPtr, 4);
                    wasm.exports.wpg_free(outLenPtr, 4);
                }
            } catch (err) {
                console.error('Failed to load WPG bytes:', err);
            }
        }
    }

    if (!customElements.get('wpg-img')) {
        customElements.define('wpg-img', WpgImageElement);
    }
    window.WpgImageElement = WpgImageElement;
})();
'@

$outPath = Join-Path $PSScriptRoot "..\playground\wpg-bundle.js"
$jsFinal = $jsTemplate.Replace('###WASM_B64###', $wasmB64)
[System.IO.File]::WriteAllText($outPath, $jsFinal, [System.Text.Encoding]::UTF8)
Write-Host "Created playground/wpg-bundle.js (Bytes: $($jsFinal.Length))"
