import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale, setLocale, t } from './i18n';

describe('i18n store', () => {
	beforeEach(() => {
		const values = new Map<string, string>();
		vi.stubGlobal('localStorage', {
			getItem: (key: string) => values.get(key) ?? null,
			setItem: (key: string, value: string) => values.set(key, value),
			clear: () => values.clear()
		});
		setLocale('en');
	});

	it('defaults to English messages', () => {
		expect(get(locale)).toBe('en');
		expect(get(t)('nav.ledger')).toBe('Ledger');
	});

	it('switches to Chinese and persists the locale', () => {
		setLocale('zh');
		expect(get(locale)).toBe('zh');
		expect(localStorage.getItem('cobrust-studio-locale')).toBe('zh');
		expect(get(t)('nav.ledger')).toBe('账本');
	});

	it('interpolates named values', () => {
		expect(get(t)('ledger.subtitle', { count: 3 })).toContain('3 rows shown');
		setLocale('zh');
		expect(get(t)('ledger.subtitle', { count: 3 })).toContain('当前显示 3 行');
	});
});
