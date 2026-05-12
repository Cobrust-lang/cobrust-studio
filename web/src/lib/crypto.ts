/**
 * Client-side WebCrypto stub for the M2 auth flow.
 *
 * Per ADR-0003 the server treats the credential triple as opaque
 * ciphertext; the real AEAD scheme (argon2id-derived key + AES-GCM-256
 * over a versioned envelope) lands at M3. For M2 we encrypt with a
 * placeholder browser-derived key so the wire shape exercises the
 * `{ciphertext, nonce, scheme}` path end-to-end. The scheme tag
 * `"aes-gcm-256/m2-stub"` advertises the limitation.
 *
 * Security caveat: the M2 stub key is derived from a single fixed
 * passphrase, so the ciphertext is recoverable by anyone with the
 * source — this is intentional placeholder behaviour, not a leak.
 * Production users MUST upgrade to M3 before storing real API keys.
 */

const SCHEME = 'aes-gcm-256/m2-stub';

/** Fixed M2 stub passphrase. M3 will replace this with a user-supplied secret. */
const STUB_PASSPHRASE = 'cobrust-studio-m2-stub-v1';

/** PBKDF2 salt — fixed for the M2 stub, deterministic across browsers. */
const STUB_SALT = new TextEncoder().encode('cobrust-studio-m2-salt-v1');

/** Derive the M2 stub AES-GCM key via PBKDF2 (100k rounds, SHA-256). */
async function deriveKey(): Promise<CryptoKey> {
	const baseKey = await crypto.subtle.importKey(
		'raw',
		new TextEncoder().encode(STUB_PASSPHRASE),
		'PBKDF2',
		false,
		['deriveKey']
	);
	return crypto.subtle.deriveKey(
		{
			name: 'PBKDF2',
			salt: STUB_SALT,
			iterations: 100_000,
			hash: 'SHA-256'
		},
		baseKey,
		{ name: 'AES-GCM', length: 256 },
		false,
		['encrypt', 'decrypt']
	);
}

/** Base64-encode a `Uint8Array` (browser-safe). */
function toBase64(bytes: Uint8Array): string {
	let binary = '';
	for (const byte of bytes) binary += String.fromCharCode(byte);
	return btoa(binary);
}

/** AES-GCM-256 encrypt a UTF-8 payload; returns the `{ciphertext, nonce, scheme}` triple. */
export async function encryptEndpointBlob(payload: {
	base_url: string;
	api_key: string;
	model: string;
}): Promise<{ ciphertext: string; nonce: string; scheme: string }> {
	const key = await deriveKey();
	const nonce = crypto.getRandomValues(new Uint8Array(12));
	const plain = new TextEncoder().encode(JSON.stringify(payload));
	const cipher = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, key, plain);
	return {
		ciphertext: toBase64(new Uint8Array(cipher)),
		nonce: toBase64(nonce),
		scheme: SCHEME
	};
}
