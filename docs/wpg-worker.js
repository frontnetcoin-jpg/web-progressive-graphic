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
    // Require Wasm if Mode 1/2, FLAG_ALPHA (0x0001), or FLAG_SECONDARY_ENTROPY (0x0008)
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
 * Pure-JS deterministic Xorshift32 film grain synthesis (matches wpg_apply_film_grain).
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
 * Instantiates and caches the WPG WebAssembly decoder module on demand.
 * @param {string} [wasmUrl]
 * @returns {Promise<WebAssembly.Instance|null>}
 */
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

/**
 * Merge FLAG_ALPHA and film grain into a bitmap. The source bytes stay on the JS heap.
 * @returns {ImageBitmap}
 */
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

    // Pure-JS fast path when no Wasm alpha decompression is needed
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

        // 1. Zero-Wasm (0 KB) Fast Path for Mode 0 without Alpha or Secondary Zstd
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

        // 2. Lazy-load Wasm (~130 KB) on demand for Mode 1, Mode 2, Alpha, or Zstd
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
