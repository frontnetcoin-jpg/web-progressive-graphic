/**
 * WPG (.wpg) Next-Gen Web Component & Offline Decoder (ESM)
 */

let wpgWasmModule = null;

const DEFAULT_WASM_URL = typeof import.meta !== 'undefined' && import.meta.url 
    ? new URL('./wpg-decoder.wasm', import.meta.url).href 
    : '/wpg-decoder.wasm';

export async function getWpgWasm(wasmUrl = DEFAULT_WASM_URL) {
    if (wpgWasmModule) return wpgWasmModule;
    try {
        const resp = await fetch(wasmUrl);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const bytes = await resp.arrayBuffer();
        const { instance } = await WebAssembly.instantiate(bytes, {});
        wpgWasmModule = instance;
        return wpgWasmModule;
    } catch (e) {
        console.warn('WPG Wasm decoder unavailable:', e);
        return null;
    }
}

export async function decodeWpg(inputBuffer, options = {}) {
    const bytes = new Uint8Array(inputBuffer);
    if (bytes.length < 16 || bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) {
        throw new Error("Invalid WPG file magic");
    }

    const width = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
    const height = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
    const flags = (bytes[12] << 8) | bytes[13];
    const hasPreview = (flags & 0x0002) !== 0;
    const hasGrain = (flags & 0x0004) !== 0;

    let previewBlob = null;
    if (hasPreview && bytes.length >= 32) {
        const p = bytes.subarray(16, 32);
        const pal = [
            [p[0], p[1], p[2]],
            [p[3], p[4], p[5]],
            [p[6], p[7], p[8]],
            [p[9], p[10], p[11]]
        ];
        const canvas = document.createElement('canvas');
        canvas.width = 4;
        canvas.height = 4;
        const ctx = canvas.getContext('2d');
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
        ctx.putImageData(imgData, 0, 0);
        previewBlob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    }

    const wasm = await getWpgWasm(options.wasmUrl || '/wpg-decoder.wasm');
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

            wasm.exports.wpg_free(avifPtr, avifLen);
            wasm.exports.wpg_free(wpgPtr, bytes.length);
            wasm.exports.wpg_free(outPtrPtr, 4);
            wasm.exports.wpg_free(outLenPtr, 4);

            return { blob, width, height, previewBlob };
        }
    }

    // Fallback if needed
    if (options.fallbackApiUrl) {
        const resp = await fetch(options.fallbackApiUrl, {
            method: 'POST',
            headers: { 'Content-Type': 'application/octet-stream' },
            body: bytes
        });
        if (resp.ok) {
            const blob = await resp.blob();
            return { blob, width, height, previewBlob };
        }
    }

    throw new Error("WPG client decode failed and no fallback available");
}

export class WpgImageElement extends HTMLElement {
    static get observedAttributes() { return ['src', 'alt', 'width', 'height', 'loading', 'wasm-url']; }

    constructor() {
        super();
        this._loadedUrl = null;
        this.attachShadow({ mode: 'open' });
        this.shadowRoot.innerHTML = `
            <style>
                :host {
                    display: inline-block;
                    position: relative;
                    overflow: hidden;
                    background: #111420;
                    border-radius: 8px;
                    line-height: 0;
                }
                canvas {
                    display: block;
                    width: 100%;
                    height: 100%;
                    object-fit: cover;
                    transition: filter 0.35s cubic-bezier(0.16, 1, 0.3, 1), transform 0.35s cubic-bezier(0.16, 1, 0.3, 1);
                }
                .preview-mode {
                    filter: blur(14px);
                    transform: scale(1.05);
                }
                .loaded-mode {
                    filter: blur(0px);
                    transform: scale(1);
                }
            </style>
            <canvas id="cvs"></canvas>
        `;
    }

    connectedCallback() {
        const src = this.getAttribute('src');
        if (!src) return;

        if (this.getAttribute('loading') === 'lazy' && 'IntersectionObserver' in window) {
            const observer = new IntersectionObserver((entries) => {
                if (entries[0].isIntersecting) {
                    observer.disconnect();
                    this.loadWpg(src);
                }
            }, { rootMargin: '200px 0px' });
            observer.observe(this);
        } else {
            this.loadWpg(src);
        }
    }

    attributeChangedCallback(name, oldVal, newVal) {
        if (name === 'src' && oldVal !== newVal && newVal) {
            this.loadWpg(newVal);
        }
    }

    async loadWpg(url) {
        if (!url || this._loadedUrl === url) return;
        this._loadedUrl = url;

        const cvs = this.shadowRoot.getElementById('cvs');
        const ctx = cvs.getContext('2d');
        cvs.className = 'preview-mode';

        try {
            const resp = await fetch(url);
            if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
            const buffer = await resp.arrayBuffer();
            const bytes = new Uint8Array(buffer);

            const decoded = await decodeWpg(bytes, {
                wasmUrl: this.getAttribute('wasm-url') || '/wpg-decoder.wasm',
                fallbackApiUrl: '/api/decode'
            });

            cvs.width = decoded.width;
            cvs.height = decoded.height;

            if (decoded.previewBlob && window.createImageBitmap) {
                const pBmp = await createImageBitmap(decoded.previewBlob);
                ctx.imageSmoothingEnabled = true;
                ctx.drawImage(pBmp, 0, 0, decoded.width, decoded.height);
            }

            if (window.createImageBitmap) {
                const bmp = await createImageBitmap(decoded.blob);
                ctx.drawImage(bmp, 0, 0);
                cvs.className = 'loaded-mode';
            }
        } catch (e) {
            console.error('Failed to load WPG image:', e);
        }
    }
}

if (typeof customElements !== 'undefined' && !customElements.get('wpg-img')) {
    customElements.define('wpg-img', WpgImageElement);
}
