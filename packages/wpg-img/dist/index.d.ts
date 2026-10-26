/**
 * WPG (.wpg) Next-Generation Web Progressive Graphic Type Definitions
 */

export interface WpgHeaderInfo {
    width: number;
    height: number;
    mode: number;
    subsampling: number;
    flags: number;
    quality: number;
    hasPreview: boolean;
    hasGrain: boolean;
    grainScale?: number;
    grainSeed?: number;
}

export interface WpgDecoderOptions {
    wasmUrl?: string;
    workerUrl?: string;
    applyFilmGrain?: boolean;
    fallbackApiUrl?: string;
}

export interface WpgWorkerPoolOptions {
    maxWorkers?: number;
    workerUrl?: string;
}

/**
 * Worker Pool managing worker thread lifecycle, concurrency, and message dispatch.
 */
export interface WpgDecodeResult {
    bitmap: ImageBitmap;
    width: number;
    height: number;
}

/** Decode promise. `requestId` is set so the caller can cancel the in-flight worker job. */
export interface WpgDecodeTask extends Promise<WpgDecodeResult> {
    requestId: number;
}

export class WpgWorkerPool {
    constructor(options?: WpgWorkerPoolOptions);
    maxWorkers: number;
    workerUrl: string;
    decode(
        buffer: ArrayBuffer,
        metadata?: {
            wasmUrl?: string;
            width?: number;
            height?: number;
            flags?: number;
            hasPreview?: boolean;
            hasGrain?: boolean;
        }
    ): WpgDecodeTask;
    cancel(id: number): void;
}

/**
 * Returns the singleton WPG Worker Pool instance.
 */
export function getWpgWorkerPool(options?: WpgWorkerPoolOptions): WpgWorkerPool;

/**
 * Initializes and caches the WPG WebAssembly decoder module.
 */
export function getWpgWasm(wasmUrl?: string): Promise<WebAssembly.Instance | null>;

/**
 * Programmatically decodes a .wpg binary buffer into an HTML5 Blob or ImageData.
 */
export function decodeWpg(
    buffer: ArrayBuffer | Uint8Array,
    options?: WpgDecoderOptions
): Promise<{ blob: Blob; width: number; height: number; previewBlob?: Blob }>;

/**
 * Options for {@link renderWpgImg}. Pass `bytes` (full file or 32-byte prefix)
 * or a 16-byte `preview` plus width and height. The returned HTML paints the
 * micro-preview and reserves the aspect-ratio box before JavaScript runs.
 */
export interface WpgImgSsrOptions {
    src: string;
    alt?: string;
    width?: number;
    height?: number;
    loading?: 'lazy' | 'eager' | string;
    fallbackUrl?: string;
    wasmUrl?: string;
    workerUrl?: string;
    className?: string;
    style?: string;
    bytes?: Uint8Array | ArrayBuffer;
    preview?: Uint8Array | ArrayBuffer;
}

/**
 * Declarative Shadow DOM markup for `<wpg-img>`. Safe to call from Node.
 */
export function renderWpgImg(options: WpgImgSsrOptions): string;

/**
 * Custom element for zero-CLS micro-previews, worker decoding, and DSD adoption.
 */
export class WpgImageElement extends HTMLElement {
    src: string | null;
    alt: string | null;
    width: number | null;
    height: number | null;
    loading: 'lazy' | 'eager' | string | null;
    wasmUrl: string | null;
    workerUrl: string | null;
    /** Server decode endpoint. `null` uses `/api/decode`. `"none"` disables that tier. */
    fallbackUrl: string | null;

    loadWpg(url: string): Promise<void>;
}

declare global {
    interface HTMLElementTagNameMap {
        'wpg-img': WpgImageElement;
    }
}
