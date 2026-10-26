/**
 * WPG (Web Progressive Graphic) Dedicated Web Worker Decoder
 *
 * Offloads Wasm allocation, AVIF wrapping, palette/modular decode, alpha merge,
 * and Xorshift32 film grain off the main thread. The finished frame is transferred
 * back as an ImageBitmap.
 */

let wasmInstance = null;
let wasmLoadingPromise = null;

/**
 * Instantiates and caches the WPG WebAssembly decoder module.
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

    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    ctx.drawImage(bitmap, 0, 0);
    if (typeof bitmap.close === 'function') bitmap.close();

    const imgData = ctx.getImageData(0, 0, width, height);
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
        if (hasGrain) {
            const offset = 16 + (previewPresent ? 16 : 0);
            if (bytes.length >= offset + 3 && bytes[offset] > 0) {
                const grainScale = bytes[offset];
                const grainSeed = (bytes[offset + 1] << 8) | bytes[offset + 2];
                wasm.exports.wpg_apply_film_grain(rgbaPtr, width, height, grainScale, grainSeed);
            }
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
