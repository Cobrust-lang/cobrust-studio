/**
 * Vitest global setup — runs before each test file.
 *
 * jsdom ≥ 22 ships `globalThis.crypto.subtle` natively, but only when
 * the test file requests `environment: 'jsdom'`. In case a future
 * jsdom regression strips `subtle`, we fall back to Node's
 * `node:crypto.webcrypto` so `encryptEndpointBlob` round-trips.
 *
 * `TextEncoder` / `TextDecoder` are present in jsdom from v16+; no
 * polyfill needed here.
 */
import { webcrypto } from 'node:crypto';

if (typeof globalThis.crypto === 'undefined' || typeof globalThis.crypto.subtle === 'undefined') {
	// jsdom didn't expose subtle — supplement with Node's webcrypto.
	Object.defineProperty(globalThis, 'crypto', {
		value: webcrypto,
		writable: false,
		configurable: true
	});
}
