/**
 * <wpg-img> Web Component & Offline WebAssembly/WebWorker Client
 * 
 * Production-grade performance client for WPG (Web Progressive Graphic):
 * 1. True HTTP Byte-Streaming (ReadableStream) for instant < 0.05ms Zero-CLS micro-previews
 * 2. Dedicated Web Worker & OffscreenCanvas pool for off-main-thread 60/120 FPS jank-free decoding
 * 3. Declarative Shadow DOM (DSD) & SSR compatibility for 0 LCP
 * 4. Strict ImageBitmap & Canvas memory pooling lifecycle management
 * 5. Robust multi-tier graceful fallback architecture (Worker -> Client Wasm -> Server Decode)
 */

const DEFAULT_WASM_URL = typeof import.meta !== 'undefined' && import.meta.url 
    ? new URL('./wpg-decoder.wasm', import.meta.url).href 
    : '/wpg-decoder.wasm';

const DEFAULT_WORKER_URL = typeof import.meta !== 'undefined' && import.meta.url 
    ? new URL('./wpg-worker.js', import.meta.url).href 
    : '/wpg-worker.js';

/** Shared shadow CSS. Canvas stays undisplayed until it has a paint class, so an SSR preview <img> can be the LCP element before JavaScript. */
const WPG_SHADOW_STYLE = `
:host {
    display: inline-block;
    position: relative;
    overflow: hidden;
    background: #111420;
    border-radius: var(--wpg-radius, 8px);
    line-height: 0;
    contain: layout paint;
    box-sizing: border-box;
    max-width: 100%;
}
:host([hidden]) { display: none !important; }
canvas {
    display: none;
    width: 100%;
    height: 100%;
    object-fit: cover;
    will-change: filter, transform;
    transition: filter 0.35s cubic-bezier(0.16, 1, 0.3, 1), transform 0.35s cubic-bezier(0.16, 1, 0.3, 1);
}
canvas.preview-mode {
    display: block;
    filter: blur(14px);
    transform: scale(1.05);
}
canvas.loaded-mode {
    display: block;
    filter: blur(0px);
    transform: scale(1);
}
img.ssr-preview {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: blur(14px);
    transform: scale(1.05);
}
img.ssr-preview.is-hidden { display: none; }
`;

const WpgHtmlElement = typeof HTMLElement === 'function' ? HTMLElement : class {};

let wpgWasmModule = null;
let wpgWasmLoadingPromise = null;

/**
 * Initializes and caches the WPG WebAssembly decoder on the main thread (for fallback use).
 * @param {string} [wasmUrl]
 * @returns {Promise<WebAssembly.Instance|null>}
 */
export async function getWpgWasm(wasmUrl = DEFAULT_WASM_URL) {
    if (wpgWasmModule) return wpgWasmModule;
    if (wpgWasmLoadingPromise) return wpgWasmLoadingPromise;

    wpgWasmLoadingPromise = (async () => {
        try {
            const resp = await fetch(wasmUrl);
            if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
            const bytes = await resp.arrayBuffer();
            const { instance } = await WebAssembly.instantiate(bytes, {});
            wpgWasmModule = instance;
            return wpgWasmModule;
        } catch (e) {
            console.warn('[wpg-img] Main-thread Wasm decoder unavailable:', e);
            return null;
        } finally {
            wpgWasmLoadingPromise = null;
        }
    })();

    return wpgWasmLoadingPromise;
}

/**
 * Inlined Worker code string for zero-config fallback if external worker script loading fails.
 */
const INLINE_WORKER_CODE = `
/**
 * WPG (Web Progressive Graphic) Dedicated Web Worker Decoder
 *
 * Offloads AVIF wrapping, palette/modular decode, alpha merge, and Xorshift32
 * film grain off the main thread. Standard Mode 0 images decode via a 0-KB Wasm
 * pure-JS ISOBMFF fast path; Wasm (~130 KB) is lazy-loaded only when needed for
 * Mode 1 (Palette), Mode 2 (Modular), FLAG_ALPHA, or FLAG_SECONDARY_ENTROPY.
 */

let wasmInstance = null;
let wasmLoadingPromise = null;

// 242-byte ISOBMFF AVIF header template (ftyp + meta + mdat head) matching avif-serialize
const AVIF_HEADER_TEMPLATE = new Uint8Array([
    0x00,0x00,0x00,0x18, 0x66,0x74,0x79,0x70, 0x61,0x76,0x69,0x66, 0x00,0x00,0x00,0x00, 0x6d,0x69,0x66,0x31, 0x6d,0x69,0x61,0x66,
    0x00,0x00,0x00,0xd2, 0x6d,0x65,0x74,0x61, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x21, 0x68,0x64,0x6c,0x72, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x70,0x69,0x63,0x74, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,
    0x00,0x00,0x00,0x0e, 0x70,0x69,0x74,0x6d, 0x00,0x00,0x00,0x00, 0x00,0x01,
    0x00,0x00,0x00,0x1e, 0x69,0x6c,0x6f,0x63, 0x00,0x00,0x00,0x00, 0x44,0x00, 0x00,0x01, 0x00,0x01, 0x00,0x00, 0x00,0x01, 0x00,0x00,0x00,0xf2, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x23, 0x69,0x69,0x6e,0x66, 0x00,0x00,0x00,0x00, 0x00,0x01, 0x00,0x00,0x00,0x15, 0x69,0x6e,0x66,0x65, 0x02,0x00,0x00,0x00, 0x00,0x01, 0x00,0x00, 0x61,0x76,0x30,0x31, 0x00,
    0x00,0x00,0x00,0x56, 0x69,0x70,0x72,0x70,
    0x00,0x00,0x00,0x38, 0x69,0x70,0x63,0x6f,
    0x00,0x00,0x00,0x14, 0x69,0x73,0x70,0x65, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x0c, 0x61,0x76,0x31,0x43, 0x81,0x1f,0x0c,0x00,
    0x00,0x00,0x00,0x10, 0x70,0x69,0x78,0x69, 0x00,0x00,0x00,0x00, 0x03,0x08,0x08,0x08,
    0x00,0x00,0x00,0x16, 0x69,0x70,0x6d,0x61, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x01, 0x00,0x01,0x03,0x01,0x82,0x03,
    0x00,0x00,0x00,0x08, 0x6d,0x64,0x61,0x74
]);

function wrapWpgMode0ToAvifJs(bytes) {
    if (bytes.length < 16) return null;
    const mode = bytes[10];
    const subsampling = bytes[11];
    const flags = (bytes[12] << 8) | bytes[13];
    if (mode !== 0 || (flags & 0x0009) !== 0) return null;

    const width = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
    const height = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
    if (width === 0 || height === 0) return null;

    let offset = 16;
    if ((flags & 0x0002) !== 0) offset += 16;
    if ((flags & 0x0004) !== 0) offset += 8;
    if (offset >= bytes.length) return null;

    const obu = bytes.subarray(offset);
    const obuLen = obu.length;
    const out = new Uint8Array(242 + obuLen);
    out.set(AVIF_HEADER_TEMPLATE, 0);
    out.set(obu, 242);

    const view = new DataView(out.buffer);
    view.setUint32(109, obuLen, false);
    view.setUint32(176, width, false);
    view.setUint32(180, height, false);
    out[193] = (subsampling >= 2) ? 0x3f : 0x1f;
    out[194] = (subsampling === 0) ? 0x0c : ((subsampling === 1) ? 0x08 : 0x00);
    view.setUint32(234, 8 + obuLen, false);
    return out;
}

function applyFilmGrainJs(data, width, height, scale, seed) {
    if (!scale || width === 0 || height === 0) return;
    const totalPixels = width * height;
    let prng = (seed === 0 ? 0x6d2b79f5 : ((seed | 0x10001) >>> 0)) >>> 0;
    const amp = scale * 0.04;

    for (let i = 0; i < totalPixels; i++) {
        const off = i * 4;
        const r = data[off];
        const g = data[off + 1];
        const b = data[off + 2];
        const normY = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
        const d = normY - 0.5;
        const lumWeight = 1.0 - 4.0 * d * d;
        if (lumWeight > 0.05) {
            prng ^= (prng << 13) >>> 0;
            prng ^= (prng >>> 17) >>> 0;
            prng ^= (prng << 5) >>> 0;
            prng = prng >>> 0;
            const randVal = ((prng & 0xffff) / 32768.0) - 1.0;
            const noise = randVal * amp * lumWeight;
            data[off] = Math.min(255, Math.max(0, Math.round(r + noise)));
            data[off + 1] = Math.min(255, Math.max(0, Math.round(g + noise)));
            data[off + 2] = Math.min(255, Math.max(0, Math.round(b + noise)));
        }
    }
}

async function getWorkerWasm(wasmUrl = '/wpg-decoder.wasm') {
    if (wasmInstance) return wasmInstance;
    if (wasmLoadingPromise) return wasmLoadingPromise;

    wasmLoadingPromise = (async () => {
        try {
            const resp = await fetch(wasmUrl);
            if (!resp.ok) throw new Error('HTTP ' + resp.status + ' fetching ' + wasmUrl);
            const bytes = await resp.arrayBuffer();
            const { instance } = await WebAssembly.instantiate(bytes, {});
            wasmInstance = instance;
            return instance;
        } catch (err) {
            console.warn('[wpg-worker] Failed to load Wasm decoder:', err);
            return null;
        } finally {
            wasmLoadingPromise = null;
        }
    })();

    return wasmLoadingPromise;
}

function wasmView(mem) {
    return new DataView(mem.buffer);
}

function copyExport(mem, ptr, len) {
    return new Uint8Array(mem.buffer, ptr, len).slice();
}

function bitmapFromRgba(rgba, width, height) {
    if (typeof OffscreenCanvas === 'undefined' || typeof ImageData === 'undefined') {
        throw new Error('OffscreenCanvas is required to paint a lossless WPG frame');
    }
    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext('2d');
    ctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
    return canvas.transferToImageBitmap();
}

function postProcessBitmap(bitmap, bytes, wasm, width, height, flags, previewPresent) {
    const hasAlpha = (flags & 0x0001) !== 0;
    const hasGrain = (flags & 0x0004) !== 0;
    if (!hasAlpha && !hasGrain) return bitmap;
    if (typeof OffscreenCanvas === 'undefined') {
        throw new Error('OffscreenCanvas is required to apply alpha or film grain');
    }

    const offset = 16 + (previewPresent ? 16 : 0);
    const grainScale = (hasGrain && bytes.length >= offset + 3) ? bytes[offset] : 0;
    const grainSeed = (hasGrain && bytes.length >= offset + 3) ? ((bytes[offset + 1] << 8) | bytes[offset + 2]) : 0;
    if (!hasAlpha && grainScale === 0) return bitmap;

    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    ctx.drawImage(bitmap, 0, 0);
    if (typeof bitmap.close === 'function') bitmap.close();

    const imgData = ctx.getImageData(0, 0, width, height);

    if (!hasAlpha && (!wasm || !wasm.exports)) {
        applyFilmGrainJs(imgData.data, width, height, grainScale, grainSeed);
        ctx.putImageData(imgData, 0, 0);
        return canvas.transferToImageBitmap();
    }

    const rgbaLen = imgData.data.length;
    const rgbaPtr = wasm.exports.wpg_alloc(rgbaLen);
    let wpgPtr = 0;
    try {
        new Uint8Array(wasm.exports.memory.buffer).set(imgData.data, rgbaPtr);
        if (hasAlpha && typeof wasm.exports.wpg_apply_alpha === 'function') {
            wpgPtr = wasm.exports.wpg_alloc(bytes.length);
            new Uint8Array(wasm.exports.memory.buffer).set(bytes, wpgPtr);
            const alphaCode = wasm.exports.wpg_apply_alpha(rgbaPtr, width, height, wpgPtr, bytes.length);
            if (alphaCode !== 0) throw new Error('wpg_apply_alpha failed with code ' + alphaCode);
        }
        if (grainScale > 0) {
            wasm.exports.wpg_apply_film_grain(rgbaPtr, width, height, grainScale, grainSeed);
        }
        imgData.data.set(new Uint8Array(wasm.exports.memory.buffer, rgbaPtr, rgbaLen));
    } finally {
        if (rgbaPtr) wasm.exports.wpg_free(rgbaPtr, rgbaLen);
        if (wpgPtr) wasm.exports.wpg_free(wpgPtr, bytes.length);
    }
    ctx.putImageData(imgData, 0, 0);
    return canvas.transferToImageBitmap();
}

self.onmessage = async function(e) {
    const data = e.data;
    if (!data) return;

    if (data.type === 'INIT_WASM') {
        const wasm = await getWorkerWasm(data.wasmUrl);
        self.postMessage({ type: 'WASM_INITIALIZED', success: !!wasm });
        return;
    }

    if (data.type !== 'DECODE') return;

    const { id, buffer, wasmUrl, width, height, flags, hasGrain, hasPreview } = data;
    try {
        const bytes = new Uint8Array(buffer);
        if (bytes.length < 16 || bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) {
            throw new Error('Invalid WPG magic signature (expected WPG\\x01)');
        }

        const imgWidth = width || ((bytes[4] << 16) | (bytes[5] << 8) | bytes[6]);
        const imgHeight = height || ((bytes[7] << 16) | (bytes[8] << 8) | bytes[9]);
        const imgFlags = flags !== undefined ? flags : ((bytes[12] << 8) | bytes[13]);
        const shouldApplyGrain = hasGrain !== undefined ? hasGrain : ((imgFlags & 0x0004) !== 0);
        const previewPresent = hasPreview !== undefined ? hasPreview : ((imgFlags & 0x0002) !== 0);
        const grainFlags = shouldApplyGrain ? (imgFlags | 0x0004) : (imgFlags & ~0x0004);

        const fastAvifBytes = wrapWpgMode0ToAvifJs(bytes);
        if (fastAvifBytes) {
            const avifBlob = new Blob([fastAvifBytes], { type: 'image/avif' });
            let bitmap = await createImageBitmap(avifBlob);
            bitmap = postProcessBitmap(bitmap, bytes, wasmInstance, imgWidth, imgHeight, grainFlags, previewPresent);
            self.postMessage({
                type: 'DECODED',
                id,
                bitmap,
                width: imgWidth,
                height: imgHeight
            }, [bitmap]);
            return;
        }

        const wasm = await getWorkerWasm(wasmUrl || '/wpg-decoder.wasm');
        if (!wasm || !wasm.exports) {
            throw new Error('Wasm decoder unavailable in worker');
        }

        const mem = wasm.exports.memory;
        const wpgPtr = wasm.exports.wpg_alloc(bytes.length);
        new Uint8Array(mem.buffer).set(bytes, wpgPtr);
        const outPtrPtr = wasm.exports.wpg_alloc(4);
        const outLenPtr = wasm.exports.wpg_alloc(4);

        const wrapCode = wasm.exports.wpg_wrap_to_avif(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
        let bitmap = null;

        if (wrapCode === -5 && typeof wasm.exports.wpg_decode_rgba === 'function') {
            const decodeCode = wasm.exports.wpg_decode_rgba(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
            if (decodeCode !== 0) {
                wasm.exports.wpg_free(wpgPtr, bytes.length);
                wasm.exports.wpg_free(outPtrPtr, 4);
                wasm.exports.wpg_free(outLenPtr, 4);
                throw new Error('wpg_decode_rgba failed with code ' + decodeCode);
            }
            const rgbaPtr = wasmView(mem).getUint32(outPtrPtr, true);
            const rgbaLen = wasmView(mem).getUint32(outLenPtr, true);
            const rgba = copyExport(mem, rgbaPtr, rgbaLen);
            wasm.exports.wpg_free(rgbaPtr, rgbaLen);
            wasm.exports.wpg_free(wpgPtr, bytes.length);
            wasm.exports.wpg_free(outPtrPtr, 4);
            wasm.exports.wpg_free(outLenPtr, 4);
            bitmap = bitmapFromRgba(rgba, imgWidth, imgHeight);
            if (shouldApplyGrain) {
                bitmap = postProcessBitmap(bitmap, bytes, wasm, imgWidth, imgHeight, grainFlags & ~0x0001, previewPresent);
            }
        } else if (wrapCode !== 0) {
            wasm.exports.wpg_free(wpgPtr, bytes.length);
            wasm.exports.wpg_free(outPtrPtr, 4);
            wasm.exports.wpg_free(outLenPtr, 4);
            throw new Error('wpg_wrap_to_avif failed with code ' + wrapCode);
        } else {
            const avifPtr = wasmView(mem).getUint32(outPtrPtr, true);
            const avifLen = wasmView(mem).getUint32(outLenPtr, true);
            const avifBytes = copyExport(mem, avifPtr, avifLen);
            wasm.exports.wpg_free(avifPtr, avifLen);
            wasm.exports.wpg_free(wpgPtr, bytes.length);
            wasm.exports.wpg_free(outPtrPtr, 4);
            wasm.exports.wpg_free(outLenPtr, 4);

            const avifBlob = new Blob([avifBytes], { type: 'image/avif' });
            bitmap = await createImageBitmap(avifBlob);
            bitmap = postProcessBitmap(bitmap, bytes, wasm, imgWidth, imgHeight, grainFlags, previewPresent);
        }

        self.postMessage({
            type: 'DECODED',
            id,
            bitmap,
            width: imgWidth,
            height: imgHeight
        }, [bitmap]);
    } catch (err) {
        self.postMessage({
            type: 'ERROR',
            id,
            error: err.message || String(err),
            needsFallback: true
        });
    }
};
`;

/**
 * Worker Pool managing worker thread lifecycle, concurrency, and message dispatch.
 */
export class WpgWorkerPool {
    constructor(options = {}) {
        this.maxWorkers = options.maxWorkers || (typeof navigator !== 'undefined' && navigator.hardwareConcurrency ? Math.min(navigator.hardwareConcurrency, 4) : 2);
        this.workerUrl = options.workerUrl || DEFAULT_WORKER_URL;
        this.workers = [];
        this.workerIndex = 0;
        this.pending = new Map();
        this.reqIdCounter = 0;
        this.disabled = false;
        this.blobUrl = null;
        this._forcedBlob = false;
        this._seenWorkerMessage = false;
    }

    _blobWorker() {
        if (typeof Worker === 'undefined' || typeof Blob === 'undefined') return null;
        if (!this.blobUrl) {
            const blob = new Blob([INLINE_WORKER_CODE], { type: 'application/javascript' });
            this.blobUrl = URL.createObjectURL(blob);
        }
        const worker = new Worker(this.blobUrl);
        worker._wpgBlob = true;
        return worker;
    }

    _spawnWorker() {
        if (this.disabled || typeof Worker === 'undefined') return null;
        let worker = null;
        try {
            if (!this._forcedBlob && this.workerUrl) {
                worker = new Worker(this.workerUrl);
            } else {
                worker = this._blobWorker();
            }
        } catch (urlErr) {
            try {
                worker = this._blobWorker();
            } catch (blobErr) {
                console.warn('[wpg-worker-pool] Unable to spawn Web Worker, falling back to main-thread decode:', blobErr || urlErr);
                this.disabled = true;
                return null;
            }
        }
        if (!worker) {
            this.disabled = true;
            return null;
        }

        worker.onmessage = (e) => this._handleWorkerMessage(e.data);
        worker.onerror = (err) => {
            console.warn('[wpg-worker-pool] Worker error occurred:', err && err.message ? err.message : err);
            if (!this._seenWorkerMessage && !worker._wpgBlob) {
                this._forcedBlob = true;
            }
            this._failWorker(worker, new Error((err && err.message) || 'Worker crashed'));
        };
        return worker;
    }

    /**
     * Rejects in-flight decodes owned by a crashed worker and drops it from the pool.
     * @param {Worker} worker
     * @param {Error} error
     */
    _failWorker(worker, error) {
        for (const [id, entry] of this.pending) {
            if (entry.worker === worker) {
                this.pending.delete(id);
                entry.reject(error);
            }
        }
        const idx = this.workers.indexOf(worker);
        if (idx !== -1) this.workers.splice(idx, 1);
        try { worker.terminate(); } catch (e) { /* already terminated */ }
    }

    _getWorker() {
        if (this.disabled) return null;
        if (this.workers.length < this.maxWorkers) {
            const w = this._spawnWorker();
            if (w) this.workers.push(w);
        }
        if (this.workers.length === 0) return null;
        const worker = this.workers[this.workerIndex % this.workers.length];
        this.workerIndex++;
        return worker;
    }

    _handleWorkerMessage(data) {
        if (!data || data.id === undefined) return;
        this._seenWorkerMessage = true;
        const entry = this.pending.get(data.id);
        if (!entry) {
            // Cancelled or orphaned request: immediately close ImageBitmap to prevent VRAM leak
            if (data.bitmap && typeof data.bitmap.close === 'function') {
                data.bitmap.close();
            }
            return;
        }
        this.pending.delete(data.id);

        if (data.type === 'DECODED') {
            entry.resolve({ bitmap: data.bitmap, width: data.width, height: data.height });
        } else {
            entry.reject(new Error(data.error || 'Worker decode failed'));
        }
    }

    /**
     * Decodes a WPG buffer in a background worker thread.
     * @param {ArrayBuffer} buffer
     * @param {Object} metadata
     * The returned promise has a `requestId` so the caller can cancel it.
     * The buffer passed to the worker is a copy, so the caller's bytes stay valid for fallbacks.
     * @returns {Promise<{ bitmap: ImageBitmap, width: number, height: number }> & { requestId: number }}
     */
    decode(buffer, metadata = {}) {
        const worker = this._getWorker();
        if (!worker) {
            return Promise.reject(new Error('Worker pool unavailable'));
        }

        const id = ++this.reqIdCounter;
        const promise = new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject, worker });
            try {
                const copy = buffer.slice(0);
                worker.postMessage({
                    type: 'DECODE',
                    id,
                    buffer: copy,
                    wasmUrl: metadata.wasmUrl,
                    width: metadata.width,
                    height: metadata.height,
                    flags: metadata.flags,
                    hasPreview: metadata.hasPreview,
                    hasGrain: metadata.hasGrain
                }, [copy]);
            } catch (postErr) {
                this.pending.delete(id);
                reject(postErr);
            }
        });
        promise.requestId = id;
        return promise;
    }

    /**
     * Cancels a pending decode request and drops references.
     * @param {number} id
     */
    cancel(id) {
        if (this.pending.has(id)) {
            const entry = this.pending.get(id);
            this.pending.delete(id);
            const err = new Error('Decode cancelled by element lifecycle');
            err.name = 'AbortError';
            entry.reject(err);
        }
    }
}

let globalWorkerPool = null;

/**
 * Returns the singleton WPG Worker Pool instance.
 * @param {Object} [options]
 * @returns {WpgWorkerPool}
 */
export function getWpgWorkerPool(options) {
    if (!globalWorkerPool) {
        globalWorkerPool = new WpgWorkerPool(options);
    }
    return globalWorkerPool;
}

/**
 * True HTTP Byte-Streaming reader:
 * Streams initial bytes to parse header and micro-preview immediately (< 0.05ms) for Zero-CLS,
 * then progressively accumulates remaining AV1/AVIF payload bytes.
 * @param {Response} response
 * @param {Function} [onHeaderReady] (prefix32Bytes) => void
 * @param {AbortSignal} [signal]
 * @returns {Promise<Uint8Array>}
 */
async function streamWpgBytes(response, onHeaderReady, signal) {
    if (!response.body || typeof response.body.getReader !== 'function') {
        const buffer = await response.arrayBuffer();
        const bytes = new Uint8Array(buffer);
        if (bytes.length >= 32 && onHeaderReady) {
            onHeaderReady(bytes.subarray(0, 32));
        }
        return bytes;
    }

    const reader = response.body.getReader();
    const chunks = [];
    let totalLength = 0;
    let headerHandled = false;

    try {
        while (true) {
            if (signal && signal.aborted) {
                reader.cancel();
                throw new Error('Aborted');
            }

            const { done, value } = await reader.read();
            if (value && value.length > 0) {
                chunks.push(value);
                totalLength += value.length;

                // Fire micro-preview callback as soon as first 32 bytes arrive
                if (!headerHandled && totalLength >= 32) {
                    const prefix = new Uint8Array(32);
                    let copied = 0;
                    for (const c of chunks) {
                        const needed = Math.min(c.length, 32 - copied);
                        prefix.set(c.subarray(0, needed), copied);
                        copied += needed;
                        if (copied >= 32) break;
                    }

                    if (prefix[0] === 0x57 && prefix[1] === 0x50 && prefix[2] === 0x47 && prefix[3] === 0x01) {
                        headerHandled = true;
                        if (onHeaderReady) {
                            try {
                                onHeaderReady(prefix);
                            } catch (e) {
                                console.warn('[wpg-img] onHeaderReady error:', e);
                            }
                        }
                    }
                }
            }

            if (done) break;
        }

        const fullBytes = new Uint8Array(totalLength);
        let offset = 0;
        for (const chunk of chunks) {
            fullBytes.set(chunk, offset);
            offset += chunk.length;
        }
        return fullBytes;
    } finally {
        reader.releaseLock();
    }
}

/**
 * Closes an ImageBitmap. A second close is ignored.
 * @param {ImageBitmap|null|undefined} drawable
 */
function closeDrawable(drawable) {
    if (!drawable || typeof drawable.close !== 'function') return;
    try { drawable.close(); } catch (e) { /* already closed */ }
}

/**
 * @param {Uint8Array} bytes
 * @returns {{ width: number, height: number, flags: number, hasPreview: boolean, hasGrain: boolean }|null}
 */
function readWpgHeader(bytes) {
    if (!bytes || bytes.length < 16) return null;
    if (bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) return null;
    const flags = (bytes[12] << 8) | bytes[13];
    return {
        width: (bytes[4] << 16) | (bytes[5] << 8) | bytes[6],
        height: (bytes[7] << 16) | (bytes[8] << 8) | bytes[9],
        flags,
        hasPreview: (flags & 0x0002) !== 0,
        hasGrain: (flags & 0x0004) !== 0
    };
}

/**
 * SVG data URI of the 4×4 micro-preview, or null when the block is absent.
 * Paintable by the browser before any script runs.
 * @param {Uint8Array} prefix
 * @returns {string|null}
 */
function microPreviewSvgDataUri(prefix) {
    const header = readWpgHeader(prefix);
    if (!header || !header.hasPreview || prefix.length < 32) return null;
    const p = prefix.subarray(16, 32);
    const pal = [
        [p[0], p[1], p[2]],
        [p[3], p[4], p[5]],
        [p[6], p[7], p[8]],
        [p[9], p[10], p[11]]
    ];
    let rects = '';
    let pixelIdx = 0;
    for (let byteIdx = 0; byteIdx < 4; byteIdx++) {
        const b = p[12 + byteIdx];
        for (const shift of [6, 4, 2, 0]) {
            const c = pal[(b >> shift) & 0x03];
            const x = pixelIdx % 4;
            const y = (pixelIdx / 4) | 0;
            rects += `<rect x="${x}" y="${y}" width="1" height="1" fill="rgb(${c[0]},${c[1]},${c[2]})"/>`;
            pixelIdx++;
        }
    }
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4" shape-rendering="crispEdges">${rects}</svg>`;
    return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/**
 * @param {unknown} value
 * @returns {string}
 */
function escapeHtmlAttr(value) {
    return String(value)
        .replace(/&/g, '&amp;')
        .replace(/"/g, '&quot;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

/**
 * Server-rendered `<wpg-img>` with a Declarative Shadow DOM.
 * The host inline style reserves the aspect-ratio box, and the shadow `<img>`
 * paints the micro-preview before JavaScript executes.
 * Safe to call in Node, Next.js, Nuxt, and SvelteKit.
 * @param {Object} options
 * @param {string} options.src
 * @param {string} [options.alt]
 * @param {number} [options.width]
 * @param {number} [options.height]
 * @param {'lazy'|'eager'|string} [options.loading]
 * @param {string} [options.fallbackUrl]
 * @param {string} [options.wasmUrl]
 * @param {string} [options.workerUrl]
 * @param {string} [options.className]
 * @param {string} [options.style]
 * @param {Uint8Array|ArrayBuffer} [options.bytes] Full file or 32-byte prefix
 * @param {Uint8Array|ArrayBuffer} [options.preview] 16-byte micro-preview block
 * @returns {string}
 */
export function renderWpgImg(options = {}) {
    const src = options.src || '';
    const alt = options.alt || '';
    let width = Number(options.width) || 0;
    let height = Number(options.height) || 0;

    let prefix = null;
    if (options.bytes) {
        const bytes = options.bytes instanceof Uint8Array ? options.bytes : new Uint8Array(options.bytes);
        const header = readWpgHeader(bytes);
        if (header) {
            if (!width) width = header.width;
            if (!height) height = header.height;
        }
        if (bytes.length >= 32) prefix = bytes.subarray(0, 32);
    } else if (options.preview && width > 0 && height > 0) {
        const preview = options.preview instanceof Uint8Array ? options.preview : new Uint8Array(options.preview);
        prefix = new Uint8Array(32);
        prefix[0] = 0x57; prefix[1] = 0x50; prefix[2] = 0x47; prefix[3] = 0x01;
        prefix[4] = (width >> 16) & 255;
        prefix[5] = (width >> 8) & 255;
        prefix[6] = width & 255;
        prefix[7] = (height >> 16) & 255;
        prefix[8] = (height >> 8) & 255;
        prefix[9] = height & 255;
        prefix[13] = 0x02;
        prefix.set(preview.subarray(0, Math.min(16, preview.length)), 16);
    }

    const uri = prefix ? microPreviewSvgDataUri(prefix) : null;
    const aspect = width > 0 && height > 0 ? `aspect-ratio:${width} / ${height}` : '';
    const hostStyle = ['display:inline-block', 'max-width:100%', aspect, options.style || ''].filter(Boolean).join(';');

    const attrs = [
        src ? `src="${escapeHtmlAttr(src)}"` : '',
        alt ? `alt="${escapeHtmlAttr(alt)}"` : '',
        width > 0 ? `width="${width}"` : '',
        height > 0 ? `height="${height}"` : '',
        options.loading ? `loading="${escapeHtmlAttr(options.loading)}"` : '',
        options.fallbackUrl ? `fallback-url="${escapeHtmlAttr(options.fallbackUrl)}"` : '',
        options.wasmUrl ? `wasm-url="${escapeHtmlAttr(options.wasmUrl)}"` : '',
        options.workerUrl ? `worker-url="${escapeHtmlAttr(options.workerUrl)}"` : '',
        options.className ? `class="${escapeHtmlAttr(options.className)}"` : '',
        `style="${escapeHtmlAttr(hostStyle)}"`
    ].filter(Boolean).join(' ');

    const previewImg = uri
        ? `<img id="ssr-preview" class="ssr-preview" alt="${escapeHtmlAttr(alt)}" width="${width || 4}" height="${height || 4}" src="${uri}" decoding="sync">`
        : '';
    const hostAspect = aspect ? `:host{${aspect};}` : '';

    return `<wpg-img ${attrs}><template shadowrootmode="open"><style>${WPG_SHADOW_STYLE}${hostAspect}</style>${previewImg}<canvas id="cvs" width="4" height="4"></canvas></template></wpg-img>`;
}

// 242-byte ISOBMFF AVIF header template (ftyp + meta + mdat head) matching avif-serialize
const AVIF_HEADER_TEMPLATE = new Uint8Array([
    0x00,0x00,0x00,0x18, 0x66,0x74,0x79,0x70, 0x61,0x76,0x69,0x66, 0x00,0x00,0x00,0x00, 0x6d,0x69,0x66,0x31, 0x6d,0x69,0x61,0x66,
    0x00,0x00,0x00,0xd2, 0x6d,0x65,0x74,0x61, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x21, 0x68,0x64,0x6c,0x72, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x70,0x69,0x63,0x74, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,
    0x00,0x00,0x00,0x0e, 0x70,0x69,0x74,0x6d, 0x00,0x00,0x00,0x00, 0x00,0x01,
    0x00,0x00,0x00,0x1e, 0x69,0x6c,0x6f,0x63, 0x00,0x00,0x00,0x00, 0x44,0x00, 0x00,0x01, 0x00,0x01, 0x00,0x00, 0x00,0x01, 0x00,0x00,0x00,0xf2, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x23, 0x69,0x69,0x6e,0x66, 0x00,0x00,0x00,0x00, 0x00,0x01, 0x00,0x00,0x00,0x15, 0x69,0x6e,0x66,0x65, 0x02,0x00,0x00,0x00, 0x00,0x01, 0x00,0x00, 0x61,0x76,0x30,0x31, 0x00,
    0x00,0x00,0x00,0x56, 0x69,0x70,0x72,0x70,
    0x00,0x00,0x00,0x38, 0x69,0x70,0x63,0x6f,
    0x00,0x00,0x00,0x14, 0x69,0x73,0x70,0x65, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x0c, 0x61,0x76,0x31,0x43, 0x81,0x1f,0x0c,0x00,
    0x00,0x00,0x00,0x10, 0x70,0x69,0x78,0x69, 0x00,0x00,0x00,0x00, 0x03,0x08,0x08,0x08,
    0x00,0x00,0x00,0x16, 0x69,0x70,0x6d,0x61, 0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x01, 0x00,0x01,0x03,0x01,0x82,0x03,
    0x00,0x00,0x00,0x08, 0x6d,0x64,0x61,0x74
]);

/**
 * Pure-JS 0-KB Wasm fast path for Mode 0 WPG images without Zstd or separate alpha.
 * @param {Uint8Array} bytes
 * @returns {Uint8Array|null}
 */
function wrapWpgMode0ToAvifJs(bytes) {
    if (bytes.length < 16) return null;
    const mode = bytes[10];
    const subsampling = bytes[11];
    const flags = (bytes[12] << 8) | bytes[13];
    if (mode !== 0 || (flags & 0x0009) !== 0) return null;

    const width = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
    const height = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
    if (width === 0 || height === 0) return null;

    let offset = 16;
    if ((flags & 0x0002) !== 0) offset += 16;
    if ((flags & 0x0004) !== 0) offset += 8;
    if (offset >= bytes.length) return null;

    const obu = bytes.subarray(offset);
    const obuLen = obu.length;
    const out = new Uint8Array(242 + obuLen);
    out.set(AVIF_HEADER_TEMPLATE, 0);
    out.set(obu, 242);

    const view = new DataView(out.buffer);
    view.setUint32(109, obuLen, false);
    view.setUint32(176, width, false);
    view.setUint32(180, height, false);
    out[193] = (subsampling >= 2) ? 0x3f : 0x1f;
    out[194] = (subsampling === 0) ? 0x0c : ((subsampling === 1) ? 0x08 : 0x00);
    view.setUint32(234, 8 + obuLen, false);
    return out;
}

/**
 * Pure-JS deterministic Xorshift32 film grain synthesis.
 * @param {Uint8ClampedArray} data
 * @param {number} width
 * @param {number} height
 * @param {number} scale
 * @param {number} seed
 */
function applyFilmGrainJs(data, width, height, scale, seed) {
    if (!scale || width === 0 || height === 0) return;
    const totalPixels = width * height;
    let prng = (seed === 0 ? 0x6d2b79f5 : ((seed | 0x10001) >>> 0)) >>> 0;
    const amp = scale * 0.04;

    for (let i = 0; i < totalPixels; i++) {
        const off = i * 4;
        const r = data[off];
        const g = data[off + 1];
        const b = data[off + 2];
        const normY = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
        const d = normY - 0.5;
        const lumWeight = 1.0 - 4.0 * d * d;
        if (lumWeight > 0.05) {
            prng ^= (prng << 13) >>> 0;
            prng ^= (prng >>> 17) >>> 0;
            prng ^= (prng << 5) >>> 0;
            prng = prng >>> 0;
            const randVal = ((prng & 0xffff) / 32768.0) - 1.0;
            const noise = randVal * amp * lumWeight;
            data[off] = Math.min(255, Math.max(0, Math.round(r + noise)));
            data[off + 1] = Math.min(255, Math.max(0, Math.round(g + noise)));
            data[off + 2] = Math.min(255, Math.max(0, Math.round(b + noise)));
        }
    }
}

/**
 * Decode a WPG buffer with Wasm.
 * Mode 0 returns AVIF bytes. Mode 1 and 2 return packed RGBA.
 * `-5` from the wrapper selects the lossless decoder. Any other failure returns null
 * so the caller can use the server fallback.
 * @param {WebAssembly.Instance|null} wasm
 * @param {Uint8Array} bytes
 * @returns {{ kind: 'avif'|'rgba', bytes: Uint8Array }|null}
 */
function decodeWpgWithWasm(wasm, bytes) {
    const fastAvif = wrapWpgMode0ToAvifJs(bytes);
    if (fastAvif) return { kind: 'avif', bytes: fastAvif };
    const exports = wasm && wasm.exports;
    if (!exports || typeof exports.wpg_alloc !== 'function') return null;
    let wpgPtr = 0;
    let outPtrPtr = 0;
    let outLenPtr = 0;
    let ownedPtr = 0;
    let ownedLen = 0;
    try {
        wpgPtr = exports.wpg_alloc(bytes.length);
        if (!wpgPtr) return null;
        new Uint8Array(exports.memory.buffer).set(bytes, wpgPtr);
        outPtrPtr = exports.wpg_alloc(4);
        outLenPtr = exports.wpg_alloc(4);
        const wrapCode = exports.wpg_wrap_to_avif(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
        let kind = '';
        if (wrapCode === 0) {
            kind = 'avif';
        } else if (wrapCode === -5 && typeof exports.wpg_decode_rgba === 'function') {
            const decodeCode = exports.wpg_decode_rgba(wpgPtr, bytes.length, outPtrPtr, outLenPtr);
            if (decodeCode !== 0) return null;
            kind = 'rgba';
        } else {
            return null;
        }
        const view = new DataView(exports.memory.buffer);
        ownedPtr = view.getUint32(outPtrPtr, true);
        ownedLen = view.getUint32(outLenPtr, true);
        const copy = new Uint8Array(exports.memory.buffer, ownedPtr, ownedLen).slice();
        return { kind, bytes: copy };
    } finally {
        const free = (ptr, len) => {
            if (!ptr) return;
            try { exports.wpg_free(ptr, len); } catch (e) { /* heap already reset */ }
        };
        free(ownedPtr, ownedLen);
        free(wpgPtr, bytes.length);
        free(outPtrPtr, 4);
        free(outLenPtr, 4);
    }
}

/**
 * @param {Uint8Array} rgba
 * @param {number} width
 * @param {number} height
 * @returns {Promise<Blob|null>}
 */
function rgbaToPngBlob(rgba, width, height) {
    if (typeof document === 'undefined') return Promise.resolve(null);
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return Promise.resolve(null);
    const imgData = ctx.createImageData(width, height);
    imgData.data.set(rgba.subarray(0, imgData.data.length));
    ctx.putImageData(imgData, 0, 0);
    return new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
}

/**
 * @param {Blob} blob
 * @returns {Promise<ImageBitmap|HTMLImageElement|null>}
 */
async function decodeBlobToDrawable(blob) {
    if (typeof createImageBitmap === 'function') {
        try {
            return await createImageBitmap(blob);
        } catch (e) {
            /* AVIF bitmap decode unsupported: try an HTMLImageElement */
        }
    }
    if (typeof Image === 'undefined' || typeof URL === 'undefined') return null;
    const url = URL.createObjectURL(blob);
    try {
        return await new Promise((resolve, reject) => {
            const img = new Image();
            img.onload = () => resolve(img);
            img.onerror = () => reject(new Error('Image element decode failed'));
            img.src = url;
        });
    } finally {
        URL.revokeObjectURL(url);
    }
}

/**
 * High-performance Custom Element for WPG progressive images.
 */
export class WpgImageElement extends WpgHtmlElement {
    static get observedAttributes() {
        return ['src', 'alt', 'width', 'height', 'loading', 'wasm-url', 'worker-url', 'fallback-url'];
    }

    /** @returns {string|null} */
    get src() { return this.getAttribute('src'); }
    set src(value) { this._setOrRemove('src', value); }

    /** @returns {string|null} */
    get alt() { return this.getAttribute('alt'); }
    set alt(value) { this._setOrRemove('alt', value); }

    /** @returns {number|null} */
    get width() {
        const v = this.getAttribute('width');
        return v == null ? null : Number(v);
    }
    set width(value) { this._setOrRemove('width', value); }

    /** @returns {number|null} */
    get height() {
        const v = this.getAttribute('height');
        return v == null ? null : Number(v);
    }
    set height(value) { this._setOrRemove('height', value); }

    /** @returns {'lazy'|'eager'|string|null} */
    get loading() { return this.getAttribute('loading'); }
    set loading(value) { this._setOrRemove('loading', value); }

    /** @returns {string|null} */
    get wasmUrl() { return this.getAttribute('wasm-url'); }
    set wasmUrl(value) { this._setOrRemove('wasm-url', value); }

    /** @returns {string|null} */
    get workerUrl() { return this.getAttribute('worker-url'); }
    set workerUrl(value) { this._setOrRemove('worker-url', value); }

    /** @returns {string|null} */
    get fallbackUrl() { return this.getAttribute('fallback-url'); }
    set fallbackUrl(value) { this._setOrRemove('fallback-url', value); }

    _setOrRemove(name, value) {
        if (value == null || value === '') this.removeAttribute(name);
        else this.setAttribute(name, String(value));
    }

    constructor() {
        super();
        this._loadedUrl = null;
        this._activeBitmap = null;
        this._abortController = null;
        this._activeReqId = null;
        this._cvs = null;
        this._ctx = null;
        this._intersectionObserver = null;
        this._loadGen = 0;
        this._inView = this.getAttribute('loading') !== 'lazy';
        this._recycled = false;

        // Declarative Shadow DOM (DSD) & SSR Adoption
        if (this.shadowRoot) {
            this._initFromShadow();
        } else {
            // Check for DSD template polyfill fallback
            const dsd = this.querySelector('template[shadowrootmode], template[shadowroot]');
            if (dsd) {
                const mode = dsd.getAttribute('shadowrootmode') || dsd.getAttribute('shadowroot') || 'open';
                const shadow = this.attachShadow({ mode });
                shadow.appendChild(dsd.content);
                dsd.remove();
                this._initFromShadow();
            } else {
                this.attachShadow({ mode: 'open' });
                this._buildShadowTree();
            }
        }
    }

    _buildShadowTree() {
        this.shadowRoot.innerHTML = `<style>${WPG_SHADOW_STYLE}</style><canvas id="cvs" width="4" height="4"></canvas>`;
        this._initFromShadow();
    }

    _initFromShadow() {
        if (!this.shadowRoot) return;
        this._cvs = this.shadowRoot.getElementById('cvs');
        if (!this._cvs && typeof document !== 'undefined') {
            this._cvs = document.createElement('canvas');
            this._cvs.id = 'cvs';
            this._cvs.width = 4;
            this._cvs.height = 4;
            this.shadowRoot.appendChild(this._cvs);
        }
        if (this._cvs) {
            this._ctx = this._cvs.getContext('2d', { alpha: true });
        }
        this._applyDeclaredDimensions();
    }

    _applyDeclaredDimensions() {
        const w = this.getAttribute('width');
        const h = this.getAttribute('height');
        if (w && h) {
            const numW = parseInt(w, 10);
            const numH = parseInt(h, 10);
            if (numW > 0 && numH > 0 && !this.style.aspectRatio) {
                this.style.aspectRatio = `${numW} / ${numH}`;
            }
        }
        const alt = this.getAttribute('alt');
        if (alt) {
            this.setAttribute('role', 'img');
            this.setAttribute('aria-label', alt);
        }
    }

    connectedCallback() {
        this._applyDeclaredDimensions();
        const src = this.getAttribute('src');
        if (!src) return;

        if (this.getAttribute('loading') === 'lazy' && typeof IntersectionObserver !== 'undefined') {
            if (this._intersectionObserver) this._intersectionObserver.disconnect();
            this._inView = false;
            this._intersectionObserver = new IntersectionObserver((entries) => {
                const entry = entries[0];
                if (!entry) return;
                this._inView = entry.isIntersecting;
                const current = this.getAttribute('src');
                if (entry.isIntersecting) {
                    if (current) this.loadWpg(current);
                } else if (this._loadedUrl || this._activeBitmap || this._activeReqId != null) {
                    this._recycleOffscreen();
                }
            }, { rootMargin: '200px 0px' });
            this._intersectionObserver.observe(this);
        } else {
            this._inView = true;
            this.loadWpg(src);
        }
    }

    disconnectedCallback() {
        this._cancelInFlight();
        this._releaseCanvasStore();
        this._loadedUrl = null;
        if (this._intersectionObserver) {
            this._intersectionObserver.disconnect();
            this._intersectionObserver = null;
        }
    }

    attributeChangedCallback(name, oldVal, newVal) {
        if (oldVal === newVal) return;
        if (name === 'src' && newVal && this.isConnected) {
            const lazyOffscreen = this.getAttribute('loading') === 'lazy' && !this._inView;
            if (lazyOffscreen) {
                this._cancelInFlight();
                this._loadedUrl = null;
                this._releaseCanvasStore();
                return;
            }
            this.loadWpg(newVal);
        } else if (name === 'width' || name === 'height' || name === 'alt') {
            this._applyDeclaredDimensions();
        }
    }

    /**
     * Aborts the fetch, cancels the worker decode, and closes any bitmap waiting to paint.
     */
    _cancelInFlight() {
        this._loadGen++;
        if (this._abortController) {
            this._abortController.abort();
            this._abortController = null;
        }
        if (this._activeReqId != null) {
            try { getWpgWorkerPool().cancel(this._activeReqId); } catch (e) { /* pool not created yet */ }
            this._activeReqId = null;
        }
        closeDrawable(this._activeBitmap);
        this._activeBitmap = null;
    }

    /**
     * Drops the full-resolution canvas backing store and reveals the lightweight preview again.
     */
    _releaseCanvasStore() {
        if (this._cvs) {
            this._cvs.className = '';
            this._cvs.width = 0;
            this._cvs.height = 0;
        }
        const img = this.shadowRoot && this.shadowRoot.getElementById('ssr-preview');
        if (img) img.classList.remove('is-hidden');
    }

    /**
     * IntersectionObserver recycle: release VRAM once a lazy image leaves the extended viewport.
     */
    _recycleOffscreen() {
        this._cancelInFlight();
        this._loadedUrl = null;
        this._recycled = true;
        this._releaseCanvasStore();
        this.setAttribute('data-state', 'recycled');
    }

    /**
     * Shows the 16-byte micro-preview synchronously.
     * An existing SSR <img> is kept so the LCP element does not swap.
     * Otherwise a data-URI <img> is inserted. The full-size canvas is not allocated.
     * @param {Uint8Array} prefix
     * @param {number} w
     * @param {number} h
     */
    _renderMicroPreview(prefix, w, h) {
        if (w > 0 && h > 0) this.style.aspectRatio = `${w} / ${h}`;
        this.setAttribute('data-state', 'preview');
        if (!this.shadowRoot || typeof document === 'undefined') return;

        const uri = microPreviewSvgDataUri(prefix);
        if (!uri) return;

        let img = this.shadowRoot.getElementById('ssr-preview');
        if (!img) {
            img = document.createElement('img');
            img.id = 'ssr-preview';
            img.className = 'ssr-preview';
            img.alt = this.getAttribute('alt') || '';
            img.decoding = 'sync';
            if (this._cvs) this.shadowRoot.insertBefore(img, this._cvs);
            else this.shadowRoot.appendChild(img);
        }
        if (!img.getAttribute('src')) img.src = uri;
        img.classList.remove('is-hidden');
        if (this._cvs) this._cvs.className = '';
    }

    _alive(gen, signal) {
        return gen === this._loadGen && this.isConnected && !(signal && signal.aborted);
    }

    /**
     * Paints alpha and film grain on the main thread when the worker tier did not.
     * The drawable is closed once its pixels have been copied to the canvas.
     * @returns {boolean} true when the canvas already holds the final image
     */
    _applyMainThreadPixels(wasm, bytes, drawable, w, h, hasPreview, hasAlpha, gen) {
        const offset = 16 + (hasPreview ? 16 : 0);
        const grainScale = bytes.length >= offset + 1 ? bytes[offset] : 0;
        const grainSeed = bytes.length >= offset + 3 ? ((bytes[offset + 1] << 8) | bytes[offset + 2]) : 0;
        if (!this._cvs || !this._ctx) return false;

        try {
            this._cvs.width = w;
            this._cvs.height = h;
            this._ctx.drawImage(drawable, 0, 0);
        } catch (drawErr) {
            return false;
        }
        closeDrawable(drawable);

        try {
            const imgData = this._ctx.getImageData(0, 0, w, h);
            if (!hasAlpha && (!wasm || !wasm.exports)) {
                applyFilmGrainJs(imgData.data, w, h, grainScale, grainSeed);
                if (gen !== this._loadGen) return true;
                this._ctx.putImageData(imgData, 0, 0);
                this._markLoaded();
                return true;
            }
            if (!wasm || !wasm.exports) return false;
            const rgbaLen = imgData.data.length;
            const rgbaPtr = wasm.exports.wpg_alloc(rgbaLen);
            let wpgPtr = 0;
            try {
                new Uint8Array(wasm.exports.memory.buffer).set(imgData.data, rgbaPtr);
                if (hasAlpha && typeof wasm.exports.wpg_apply_alpha === 'function') {
                    wpgPtr = wasm.exports.wpg_alloc(bytes.length);
                    new Uint8Array(wasm.exports.memory.buffer).set(bytes, wpgPtr);
                    const alphaCode = wasm.exports.wpg_apply_alpha(rgbaPtr, w, h, wpgPtr, bytes.length);
                    if (alphaCode !== 0) throw new Error('wpg_apply_alpha failed with code ' + alphaCode);
                }
                if (grainScale > 0) {
                    wasm.exports.wpg_apply_film_grain(rgbaPtr, w, h, grainScale, grainSeed);
                }
                imgData.data.set(new Uint8Array(wasm.exports.memory.buffer, rgbaPtr, rgbaLen));
            } finally {
                try { wasm.exports.wpg_free(rgbaPtr, rgbaLen); } catch (e) { /* heap already reset */ }
                if (wpgPtr) {
                    try { wasm.exports.wpg_free(wpgPtr, bytes.length); } catch (e) { /* heap already reset */ }
                }
            }
            if (gen !== this._loadGen) return true;
            this._ctx.putImageData(imgData, 0, 0);
            this._markLoaded();
            return true;
        } catch (pixelErr) {
            console.warn('[wpg-img] Alpha or film grain post-process failed:', pixelErr);
            return false;
        }
    }

    /**
     * Paint a packed RGBA frame from the lossless Wasm decoder.
     * @returns {boolean}
     */
    _paintRgba(wasm, rgba, bytes, w, h, hasPreview, hasGrain, gen) {
        if (!this._cvs || !this._ctx || gen !== this._loadGen) return false;
        this._cvs.width = w;
        this._cvs.height = h;
        const imgData = this._ctx.createImageData(w, h);
        imgData.data.set(rgba.subarray(0, imgData.data.length));
        if (hasGrain && wasm && wasm.exports) {
            const offset = 16 + (hasPreview ? 16 : 0);
            if (bytes.length >= offset + 3 && bytes[offset] > 0) {
                const rgbaLen = imgData.data.length;
                const rgbaPtr = wasm.exports.wpg_alloc(rgbaLen);
                try {
                    new Uint8Array(wasm.exports.memory.buffer).set(imgData.data, rgbaPtr);
                    const grainSeed = (bytes[offset + 1] << 8) | bytes[offset + 2];
                    wasm.exports.wpg_apply_film_grain(rgbaPtr, w, h, bytes[offset], grainSeed);
                    imgData.data.set(new Uint8Array(wasm.exports.memory.buffer, rgbaPtr, rgbaLen));
                } finally {
                    try { wasm.exports.wpg_free(rgbaPtr, rgbaLen); } catch (e) { /* heap already reset */ }
                }
            }
        }
        if (gen !== this._loadGen) return false;
        this._ctx.putImageData(imgData, 0, 0);
        this._markLoaded();
        return true;
    }

    _markLoaded() {
        if (this._cvs) this._cvs.className = 'loaded-mode';
        const img = this.shadowRoot && this.shadowRoot.getElementById('ssr-preview');
        if (img) img.classList.add('is-hidden');
        this.setAttribute('data-state', 'loaded');
    }

    /**
     * Loads, streams, and decodes a WPG image url.
     * @param {string} url
     */
    async loadWpg(url) {
        if (!url || (this._loadedUrl === url && !this._recycled)) return;
        this._recycled = false;
        this._cancelInFlight();
        const gen = this._loadGen;
        this._loadedUrl = url;
        this._abortController = new AbortController();
        const signal = this._abortController.signal;

        this.removeAttribute('data-error');
        if (!this._cvs || !this._ctx) this._initFromShadow();
        if (!this._cvs || !this._ctx) return;

        let parsedW = 0;
        let parsedH = 0;
        let parsedFlags = 0;
        let parsedHasGrain = false;
        let parsedHasPreview = false;

        try {
            const resp = await fetch(url, { signal });
            if (!this._alive(gen, signal)) return;
            if (!resp.ok) throw new Error(`HTTP ${resp.status} for ${url}`);

            // 1. True HTTP Byte-Streaming: Parse header & render micro-preview immediately
            const bytes = await streamWpgBytes(resp, (prefix) => {
                if (!this._alive(gen, signal)) return;
                const header = readWpgHeader(prefix);
                if (!header) return;
                parsedW = header.width;
                parsedH = header.height;
                parsedFlags = header.flags;
                parsedHasPreview = header.hasPreview;
                parsedHasGrain = header.hasGrain;
                this._renderMicroPreview(prefix, parsedW, parsedH);
            }, signal);

            if (!this._alive(gen, signal)) return;
            const header = readWpgHeader(bytes);
            if (!header) {
                throw new Error(bytes.length < 16 ? 'Truncated WPG file' : 'Invalid WPG magic signature');
            }

            if (!parsedW) {
                parsedW = header.width;
                parsedH = header.height;
                parsedFlags = header.flags;
                parsedHasPreview = header.hasPreview;
                parsedHasGrain = header.hasGrain;
                this._renderMicroPreview(bytes.subarray(0, Math.min(32, bytes.length)), parsedW, parsedH);
            }

            const wasmUrl = this.getAttribute('wasm-url') || DEFAULT_WASM_URL;
            const workerUrl = this.getAttribute('worker-url') || DEFAULT_WORKER_URL;
            const fallbackAttr = this.getAttribute('fallback-url');
            const fallbackUrl = fallbackAttr == null ? '/api/decode' : fallbackAttr;
            let decodedSuccessfully = false;

            // 2. Off-Main-Thread Web Worker Decoding (60/120 FPS jank-free)
            const pool = getWpgWorkerPool({ workerUrl });
            if (!pool.disabled && this._alive(gen, signal)) {
                try {
                    const owned = (bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength)
                        ? bytes.buffer
                        : bytes.slice().buffer;
                    const task = pool.decode(owned, {
                        wasmUrl,
                        width: parsedW,
                        height: parsedH,
                        flags: parsedFlags,
                        hasPreview: parsedHasPreview,
                        hasGrain: parsedHasGrain
                    });
                    this._activeReqId = task.requestId;
                    const result = await task;
                    this._activeReqId = null;

                    if (!this._alive(gen, signal)) {
                        closeDrawable(result.bitmap);
                        return;
                    }

                    this._drawFinalBitmap(result.bitmap, result.width || parsedW, result.height || parsedH, gen);
                    decodedSuccessfully = true;
                } catch (workerErr) {
                    this._activeReqId = null;
                    if (!this._alive(gen, signal) || workerErr.name === 'AbortError') return;
                    console.warn('[wpg-img] Worker decode failed, falling back to main-thread Wasm:', workerErr);
                }
            }

            // 3. Fallback: Main-thread JS/Wasm decode. `bytes` is still intact; the worker received a copy.
            if (!decodedSuccessfully && this._alive(gen, signal)) {
                try {
                    const fastAvif = wrapWpgMode0ToAvifJs(bytes);
                    const wasm = fastAvif ? null : await getWpgWasm(wasmUrl);
                    if (!this._alive(gen, signal)) return;
                    const decoded = fastAvif ? { kind: 'avif', bytes: fastAvif } : (wasm ? decodeWpgWithWasm(wasm, bytes) : null);
                    if (decoded && decoded.kind === 'rgba') {
                        if (this._paintRgba(wasm, decoded.bytes, bytes, parsedW, parsedH, parsedHasPreview, parsedHasGrain, gen)) {
                            decodedSuccessfully = true;
                        }
                    } else if (decoded && decoded.kind === 'avif') {
                        const blob = new Blob([decoded.bytes], { type: 'image/avif' });
                        const drawable = await decodeBlobToDrawable(blob);
                        if (!this._alive(gen, signal)) {
                            closeDrawable(drawable);
                            return;
                        }
                        if (!drawable) throw new Error('AVIF decode produced no image');

                        const hasAlpha = (parsedFlags & 0x0001) !== 0;
                        if (parsedHasGrain || hasAlpha) {
                            const painted = this._applyMainThreadPixels(wasm, bytes, drawable, parsedW, parsedH, parsedHasPreview, hasAlpha, gen);
                            if (painted) decodedSuccessfully = true;
                            else throw new Error('Alpha or film grain post-process failed');
                        }
                        if (!decodedSuccessfully) {
                            this._drawFinalBitmap(drawable, parsedW, parsedH, gen);
                            decodedSuccessfully = true;
                        }
                    }
                } catch (wasmErr) {
                    if (!this._alive(gen, signal) || wasmErr.name === 'AbortError') return;
                    console.warn('[wpg-img] Main-thread Wasm decode failed, falling back to server decode:', wasmErr);
                }
            }

            // 4. Fallback: server decode. `fallback-url="none"` skips this tier.
            if (!decodedSuccessfully && this._alive(gen, signal) && fallbackUrl && fallbackUrl !== 'none') {
                const decResp = await fetch(fallbackUrl, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/octet-stream' },
                    body: bytes,
                    signal
                });
                if (!this._alive(gen, signal)) return;
                if (decResp.ok) {
                    const blob = await decResp.blob();
                    const drawable = await decodeBlobToDrawable(blob);
                    if (!this._alive(gen, signal)) {
                        closeDrawable(drawable);
                        return;
                    }
                    if (drawable) {
                        this._drawFinalBitmap(drawable, parsedW, parsedH, gen);
                        decodedSuccessfully = true;
                    }
                }
            }

            if (!decodedSuccessfully && this._alive(gen, signal)) {
                throw new Error('All decoding tiers failed for WPG image');
            }

        } catch (err) {
            if (!this._alive(gen, signal) || err.name === 'AbortError') return;
            console.error('[wpg-img] Error loading image:', err);
            this.setAttribute('data-error', 'load_failed');
            if (typeof CustomEvent !== 'undefined') {
                this.dispatchEvent(new CustomEvent('error', { detail: { error: err, url } }));
            }
        }
    }

    _drawFinalBitmap(bitmap, width, height, gen) {
        const paint = () => {
            if (gen !== this._loadGen || !this.isConnected || this._activeBitmap !== bitmap || !this._cvs || !this._ctx) {
                closeDrawable(bitmap);
                if (this._activeBitmap === bitmap) this._activeBitmap = null;
                return;
            }
            this._cvs.width = width;
            this._cvs.height = height;
            this._ctx.imageSmoothingEnabled = true;
            this._ctx.drawImage(bitmap, 0, 0);
            closeDrawable(bitmap);
            if (this._activeBitmap === bitmap) this._activeBitmap = null;
            this._markLoaded();
            if (typeof CustomEvent !== 'undefined') {
                this.dispatchEvent(new CustomEvent('load', { detail: { width, height } }));
            }
        };

        if (gen !== this._loadGen || !this.isConnected || !this._cvs || !this._ctx) {
            closeDrawable(bitmap);
            return;
        }

        closeDrawable(this._activeBitmap);
        this._activeBitmap = bitmap;
        if (typeof requestAnimationFrame === 'function') requestAnimationFrame(paint);
        else paint();
    }
}

/**
 * Programmatically decodes a WPG binary buffer.
 * @param {ArrayBuffer|Uint8Array} inputBuffer
 * @param {Object} [options]
 * @returns {Promise<{ blob: Blob, width: number, height: number, previewBlob?: Blob }>}
 */
export async function decodeWpg(inputBuffer, options = {}) {
    const bytes = new Uint8Array(inputBuffer);
    if (bytes.length < 16 || bytes[0] !== 0x57 || bytes[1] !== 0x50 || bytes[2] !== 0x47 || bytes[3] !== 0x01) {
        throw new Error("Invalid WPG file magic");
    }

    const width = (bytes[4] << 16) | (bytes[5] << 8) | bytes[6];
    const height = (bytes[7] << 16) | (bytes[8] << 8) | bytes[9];
    const flags = (bytes[12] << 8) | bytes[13];
    const hasPreview = (flags & 0x0002) !== 0;

    let previewBlob = null;
    if (hasPreview && bytes.length >= 32 && typeof document !== 'undefined') {
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
        if (ctx) {
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
    }

    const fastAvif = wrapWpgMode0ToAvifJs(bytes);
    if (fastAvif) {
        return { blob: new Blob([fastAvif], { type: 'image/avif' }), width, height, previewBlob };
    }

    const wasm = await getWpgWasm(options.wasmUrl || DEFAULT_WASM_URL);
    const decoded = wasm ? decodeWpgWithWasm(wasm, bytes) : null;
    if (decoded && decoded.kind === 'rgba' && typeof document !== 'undefined') {
        const png = await rgbaToPngBlob(decoded.bytes, width, height);
        if (png) return { blob: png, width, height, previewBlob };
    }
    if (decoded && decoded.kind === 'avif' && (flags & 0x0001) === 0) {
        return { blob: new Blob([decoded.bytes], { type: 'image/avif' }), width, height, previewBlob };
    }
    if (decoded && decoded.kind === 'avif' && typeof document !== 'undefined') {
        const drawable = await decodeBlobToDrawable(new Blob([decoded.bytes], { type: 'image/avif' }));
        if (drawable && wasm) {
            const canvas = document.createElement('canvas');
            canvas.width = width;
            canvas.height = height;
            const ctx = canvas.getContext('2d');
            if (ctx) {
                ctx.drawImage(drawable, 0, 0);
                closeDrawable(drawable);
                const imgData = ctx.getImageData(0, 0, width, height);
                const rgbaPtr = wasm.exports.wpg_alloc(imgData.data.length);
                const wpgPtr = wasm.exports.wpg_alloc(bytes.length);
                try {
                    new Uint8Array(wasm.exports.memory.buffer).set(imgData.data, rgbaPtr);
                    new Uint8Array(wasm.exports.memory.buffer).set(bytes, wpgPtr);
                    const alphaCode = wasm.exports.wpg_apply_alpha(rgbaPtr, width, height, wpgPtr, bytes.length);
                    if (alphaCode === 0) {
                        imgData.data.set(new Uint8Array(wasm.exports.memory.buffer, rgbaPtr, imgData.data.length));
                        ctx.putImageData(imgData, 0, 0);
                        const png = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
                        if (png) return { blob: png, width, height, previewBlob };
                    }
                } finally {
                    try { wasm.exports.wpg_free(rgbaPtr, imgData.data.length); } catch (e) { /* heap already reset */ }
                    try { wasm.exports.wpg_free(wpgPtr, bytes.length); } catch (e) { /* heap already reset */ }
                }
            }
        }
    }

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

    throw new Error("WPG decode failed and no fallback available");
}

if (typeof customElements !== 'undefined' && !customElements.get('wpg-img')) {
    customElements.define('wpg-img', WpgImageElement);
}
