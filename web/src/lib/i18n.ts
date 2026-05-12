import { derived, writable } from 'svelte/store';
import { en } from '$lib/i18n/en';
import { zh } from '$lib/i18n/zh';

export type Locale = 'en' | 'zh';
export type MessageKey = keyof typeof en;

type Params = Record<string, string | number>;

const messages: Record<Locale, Record<MessageKey, string>> = { en, zh };
const STORAGE_KEY = 'cobrust-studio-locale';

function canPersistLocale(): boolean {
	return (
		typeof localStorage !== 'undefined' &&
		typeof localStorage.getItem === 'function' &&
		typeof localStorage.setItem === 'function'
	);
}

function initialLocale(): Locale {
	if (!canPersistLocale()) return 'en';
	return localStorage.getItem(STORAGE_KEY) === 'zh' ? 'zh' : 'en';
}

export const locale = writable<Locale>(initialLocale());

export const t = derived(locale, ($locale) => {
	return (key: MessageKey, params: Params = {}) => {
		let message = messages[$locale][key] ?? en[key] ?? key;
		for (const [name, value] of Object.entries(params)) {
			message = message.replaceAll(`{${name}}`, String(value));
		}
		return message;
	};
});

export function setLocale(next: Locale) {
	locale.set(next);
	if (canPersistLocale()) localStorage.setItem(STORAGE_KEY, next);
}
