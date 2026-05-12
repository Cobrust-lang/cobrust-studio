/**
 * Unit tests for the M2 client-side WebCrypto stub.
 *
 * Pins:
 * - The `{ ciphertext, nonce, scheme }` wire shape exported by
 *   `encryptEndpointBlob` matches `EncryptedBlob` in `types.ts`.
 * - The literal scheme tag `"aes-gcm-256/m2-stub"` (advertised by the
 *   server-side credential-blob handler and documented in ADR-0003).
 *   If this changes the server must change in lockstep — fail loud.
 * - The deterministic PBKDF2-derived key successfully decrypts what
 *   it encrypted, with the same scheme + nonce path. (The exported
 *   surface is encrypt-only; we exercise decrypt via the WebCrypto
 *   primitives directly to assert the inverse holds.)
 * - Tampered ciphertext fails authentication (AES-GCM AEAD tag check)
 *   — i.e. the stub is not silently lossy.
 *
 * Anchor: docs/agent/modules/web-frontend.md §"Auth (ADR-0003)".
 */

import { describe, expect, it } from 'vitest';
import { encryptEndpointBlob } from './crypto';
import type { EncryptedBlob } from './types';

// ───── Helpers ──────────────────────────────────────────────────────

/**
 * Decode a browser-style base64 string back to bytes.
 *
 * Returns `Uint8Array<ArrayBuffer>` (not `ArrayBufferLike`) so the
 * result is assignable to `BufferSource` under strict TS — WebCrypto
 * APIs reject the wider `ArrayBufferLike`-backed variant.
 */
function fromBase64(b64: string): Uint8Array<ArrayBuffer> {
	const binary = atob(b64);
	const buf = new ArrayBuffer(binary.length);
	const out = new Uint8Array(buf);
	for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
	return out;
}

/**
 * Re-derive the same PBKDF2 key that `crypto.ts` builds, using the
 * documented passphrase + salt + iteration parameters from that file.
 *
 * This is duplicated on purpose: if a future PR changes the derivation
 * parameters, these tests fail and surface that the wire-shape changed.
 */
async function deriveStubKey(): Promise<CryptoKey> {
	const baseKey = await crypto.subtle.importKey(
		'raw',
		new TextEncoder().encode('cobrust-studio-m2-stub-v1'),
		'PBKDF2',
		false,
		['deriveKey']
	);
	return crypto.subtle.deriveKey(
		{
			name: 'PBKDF2',
			salt: new TextEncoder().encode('cobrust-studio-m2-salt-v1'),
			iterations: 100_000,
			hash: 'SHA-256'
		},
		baseKey,
		{ name: 'AES-GCM', length: 256 },
		false,
		['encrypt', 'decrypt']
	);
}

// ───── Tests ────────────────────────────────────────────────────────

describe('encryptEndpointBlob — wire shape', () => {
	it('returns the EncryptedBlob triple { ciphertext, nonce, scheme }', async () => {
		const out = await encryptEndpointBlob({
			base_url: 'https://api.anthropic.com',
			api_key: 'sk-test-abc',
			model: 'claude-opus-4-7'
		});
		// Compile-time: shape lines up with the wire contract.
		const _typecheck: EncryptedBlob = out;
		void _typecheck;
		expect(typeof out.ciphertext).toBe('string');
		expect(typeof out.nonce).toBe('string');
		expect(typeof out.scheme).toBe('string');
		expect(out.ciphertext.length).toBeGreaterThan(0);
		expect(out.nonce.length).toBeGreaterThan(0);
	});

	it('advertises the literal scheme tag "aes-gcm-256/m2-stub"', async () => {
		const out = await encryptEndpointBlob({
			base_url: 'https://x',
			api_key: 'y',
			model: 'z'
		});
		// Server pass-through stores this verbatim; M3 AEAD upgrade must
		// either keep this string or coordinate a server-side migration.
		expect(out.scheme).toBe('aes-gcm-256/m2-stub');
	});

	it('emits a 12-byte AES-GCM nonce (96-bit, NIST-recommended)', async () => {
		const out = await encryptEndpointBlob({
			base_url: 'https://x',
			api_key: 'y',
			model: 'z'
		});
		const nonceBytes = fromBase64(out.nonce);
		expect(nonceBytes.length).toBe(12);
	});

	it('produces a fresh nonce per call (non-deterministic envelope)', async () => {
		const payload = { base_url: 'https://x', api_key: 'y', model: 'z' };
		const a = await encryptEndpointBlob(payload);
		const b = await encryptEndpointBlob(payload);
		expect(a.nonce).not.toBe(b.nonce);
		expect(a.ciphertext).not.toBe(b.ciphertext);
	});
});

describe('encryptEndpointBlob — round trip', () => {
	it('decrypts back to the exact JSON payload (UTF-8 verbatim)', async () => {
		const payload = {
			base_url: 'https://api.anthropic.com',
			api_key: 'sk-ant-very-long-secret-token-with-symbols-!@#',
			model: 'claude-opus-4-7'
		};
		const blob = await encryptEndpointBlob(payload);
		const key = await deriveStubKey();
		const plain = await crypto.subtle.decrypt(
			{ name: 'AES-GCM', iv: fromBase64(blob.nonce) },
			key,
			fromBase64(blob.ciphertext)
		);
		const recovered = JSON.parse(new TextDecoder().decode(plain));
		expect(recovered).toEqual(payload);
	});

	it('round-trips UTF-8 / non-ASCII bytes safely', async () => {
		const payload = {
			base_url: 'https://例え.テスト',
			api_key: '密钥-🔑-καλά',
			model: 'claude-opus-4-7'
		};
		const blob = await encryptEndpointBlob(payload);
		const key = await deriveStubKey();
		const plain = await crypto.subtle.decrypt(
			{ name: 'AES-GCM', iv: fromBase64(blob.nonce) },
			key,
			fromBase64(blob.ciphertext)
		);
		expect(JSON.parse(new TextDecoder().decode(plain))).toEqual(payload);
	});

	it('tampered ciphertext fails AEAD auth (does NOT silently decrypt to empty)', async () => {
		const blob = await encryptEndpointBlob({
			base_url: 'https://x',
			api_key: 'y',
			model: 'z'
		});
		const key = await deriveStubKey();
		const tampered = fromBase64(blob.ciphertext);
		// Flip a single bit in the body (avoid the 16-byte AEAD tag suffix to
		// be sure we corrupt the message, not just the tag).
		tampered[0] = tampered[0] ^ 0x01;
		await expect(
			crypto.subtle.decrypt({ name: 'AES-GCM', iv: fromBase64(blob.nonce) }, key, tampered)
		).rejects.toThrow();
	});

	it('wrong nonce fails auth (proves the nonce is part of the AEAD envelope)', async () => {
		const blob = await encryptEndpointBlob({
			base_url: 'https://x',
			api_key: 'y',
			model: 'z'
		});
		const key = await deriveStubKey();
		// all zeros — guaranteed != random; ArrayBuffer-backed for strict TS.
		const wrongNonce = new Uint8Array(new ArrayBuffer(12));
		await expect(
			crypto.subtle.decrypt({ name: 'AES-GCM', iv: wrongNonce }, key, fromBase64(blob.ciphertext))
		).rejects.toThrow();
	});
});
