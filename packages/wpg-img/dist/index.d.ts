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
    applyFilmGrain?: boolean;
    fallbackApiUrl?: string;
}

/**
 * Initializes and caches the WPG WebAssembly decoder module
 */
export function getWpgWasm(wasmUrl?: string): Promise<WebAssembly.Instance | null>;

/**
 * Programmatically decodes a .wpg binary buffer into an HTML5 Blob or ImageData
 */
export function decodeWpg(
    buffer: ArrayBuffer | Uint8Array,
    options?: WpgDecoderOptions
): Promise<{ blob: Blob; width: number; height: number; previewBlob?: Blob }>;

/**
 * Custom HTML element for displaying .wpg images with instant zero-CLS micro-preview
 */
export class WpgImageElement extends HTMLElement {
    src: string | null;
    alt: string | null;
    width: number | null;
    height: number | null;
    wasmUrl: string | null;

    loadWpg(url: string): Promise<void>;
}

declare global {
    interface HTMLElementTagNameMap {
        'wpg-img': WpgImageElement;
    }
}
