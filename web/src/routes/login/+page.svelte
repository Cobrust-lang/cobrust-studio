<!--
	/login — custom endpoint + API key per ADR-0003 + ADR-0007 M6 + ADR-0008 M7.

	Two tabs visually: "API key" (active, drives the flow) and "OAuth"
	(greyed out with a "Coming v0.5.0" placeholder). The form POSTs
	`{endpoint, api_key, model, passphrase, provider_kind}` plaintext over
	TLS/localhost to `/api/login`; the server runs Argon2id(passphrase) →
	32-byte key, AES-256-GCM-seals the credentials, persists the blob in
	session_kv, and holds the derived `SessionKey` in memory for the binary
	lifetime (per ADR-0007). On binary restart the passphrase must be
	re-entered.

	M7 addition (ADR-0008): a `<select>` for provider type + URL-based
	auto-suggest reactive logic. The dropdown defaults to "anthropic" and
	auto-suggests based on the base URL as the user types. The user can
	override the suggestion.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import Button from '$lib/components/Button.svelte';
	import { login, getVersion, previewModels, ApiError } from '$lib/api';
	import { locale, setLocale, t } from '$lib/i18n';
	import type { Locale } from '$lib/i18n';
	import { cn } from '$lib/util';

	let tab = $state<'api' | 'oauth'>('api');
	let baseUrl = $state('https://api.anthropic.com');
	let apiKey = $state('');
	let model = $state('');
	let passphrase = $state('');
	let providerKind = $state<'anthropic' | 'openai'>('anthropic');
	let modelOptions = $state<string[]>([]);
	let loadingModels = $state(false);
	let submitting = $state(false);
	let toast = $state<{ kind: 'ok' | 'err'; msg: string } | null>(null);

	let version = $state<string>('…');
	onMount(async () => {
		try {
			const v = await getVersion();
			version = v.studio_server;
		} catch {
			version = 'unknown';
		}
	});

	function chooseLocale(next: Locale) {
		setLocale(next);
	}

	function clearDiscoveredModels() {
		modelOptions = [];
		model = '';
	}

	function handleBaseUrlInput() {
		if (baseUrl.includes('anthropic.com')) {
			providerKind = 'anthropic';
		} else if (baseUrl.length > 0 && !baseUrl.includes('anthropic.com')) {
			providerKind = 'openai';
		}
		clearDiscoveredModels();
	}

	function handleProviderChange() {
		clearDiscoveredModels();
	}

	async function fetchModels() {
		toast = null;
		if (!baseUrl.trim() || !apiKey.trim()) {
			toast = { kind: 'err', msg: $t('login.errFetchModelsFields') };
			return;
		}
		loadingModels = true;
		try {
			const resp = await previewModels({
				endpoint: baseUrl.trim(),
				api_key: apiKey.trim(),
				provider_kind: providerKind
			});
			modelOptions = resp.models;
			model = resp.models[0] ?? '';
			toast = { kind: 'ok', msg: $t('login.modelsLoaded', { count: resp.models.length }) };
		} catch (e) {
			const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
			toast = { kind: 'err', msg };
		} finally {
			loadingModels = false;
		}
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		toast = null;
		if (!baseUrl.trim() || !apiKey.trim() || !model.trim() || !passphrase) {
			toast = {
				kind: 'err',
				msg: model.trim() ? $t('login.errAllFields') : $t('login.errModelRequired')
			};
			return;
		}
		if (passphrase.length < 8) {
			toast = { kind: 'err', msg: $t('login.errPassphraseShort') };
			return;
		}
		submitting = true;
		try {
			await login({
				endpoint: baseUrl.trim(),
				api_key: apiKey.trim(),
				model: model.trim(),
				passphrase,
				provider_kind: providerKind
			});
			toast = { kind: 'ok', msg: $t('login.okUnlocked') };
			setTimeout(() => goto('/adr'), 400);
		} catch (e) {
			const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
			toast = { kind: 'err', msg };
		} finally {
			submitting = false;
		}
	}
</script>

<div class="relative grid min-h-screen place-items-center px-4">
	<div class="absolute right-4 top-4 flex rounded-md bg-secondary p-0.5 text-xs">
		<button
			type="button"
			onclick={() => chooseLocale('en')}
			class={cn(
				'rounded px-1.5 py-0.5 text-[0.65rem] font-medium',
				$locale === 'en' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground'
			)}
		>
			EN
		</button>
		<button
			type="button"
			onclick={() => chooseLocale('zh')}
			class={cn(
				'rounded px-1.5 py-0.5 text-[0.65rem] font-medium',
				$locale === 'zh' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground'
			)}
		>
			中
		</button>
	</div>
	<div class="w-full max-w-md">
		<div class="mb-6 text-center">
			<div
				class="mx-auto mb-3 grid h-10 w-10 place-items-center rounded-lg bg-primary text-primary-foreground font-mono text-sm font-bold"
			>
				CS
			</div>
			<h1 class="text-xl font-semibold">Cobrust Studio</h1>
			<p class="mt-1 text-xs text-muted-foreground">
				{$t('login.subtitle')}
			</p>
		</div>

		<div class="rounded-lg border border-border bg-card p-5 shadow-sm">
			<div class="mb-4 flex gap-1 rounded-md bg-secondary p-1 text-xs">
				<button
					type="button"
					onclick={() => (tab = 'api')}
					class={cn(
						'flex-1 rounded px-2 py-1.5 font-medium transition-colors',
						tab === 'api' ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground'
					)}
				>
					{$t('login.apiKeyTab')}
				</button>
				<button
					type="button"
					disabled
					class="flex-1 rounded px-2 py-1.5 text-muted-foreground/60 cursor-not-allowed"
					title={$t('login.oauthTitle')}
				>
					{$t('login.oauthTab')} <span class="ml-1 text-[0.65rem] opacity-70">(v0.5.0)</span>
				</button>
			</div>

			{#if tab === 'api'}
				<form onsubmit={submit} class="flex flex-col gap-3">
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">{$t('login.baseUrl')}</span>
						<input
							type="text"
							bind:value={baseUrl}
							oninput={handleBaseUrlInput}
							placeholder="https://api.anthropic.com"
							autocomplete="off"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
						/>
						<span class="text-[0.65rem] text-muted-foreground/80 leading-snug">
							{$t('login.baseUrlHelp')}
						</span>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">{$t('login.apiKey')}</span>
						<input
							type="password"
							bind:value={apiKey}
							oninput={clearDiscoveredModels}
							autocomplete="off"
							placeholder="sk-…"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">{$t('common.provider')}</span>
						<select
							bind:value={providerKind}
							onchange={handleProviderChange}
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
						>
							<option value="anthropic">Anthropic API</option>
							<option value="openai"
								>OpenAI-compatible (vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama)</option
							>
						</select>
					</label>
					<div class="flex flex-col gap-1 text-sm">
						<div class="flex items-center justify-between gap-2">
							<span class="text-xs text-muted-foreground">{$t('common.model')}</span>
							<Button type="button" variant="ghost" disabled={loadingModels} onclick={fetchModels}>
								{loadingModels ? $t('login.fetchingModels') : $t('login.fetchModels')}
							</Button>
						</div>
						<input
							type="text"
							bind:value={model}
							list="login-model-options"
							placeholder={$t('login.modelPlaceholder')}
							autocomplete="off"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
						<datalist id="login-model-options">
							{#each modelOptions as option}
								<option value={option}></option>
							{/each}
						</datalist>
						<span class="text-[0.65rem] text-muted-foreground/80 leading-snug">
							{$t('login.modelHelp')}
						</span>
					</div>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">{$t('login.passphrase')}</span>
						<input
							type="password"
							bind:value={passphrase}
							autocomplete="new-password"
							placeholder={$t('login.passphrasePlaceholder')}
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
					</label>
					<p class="text-[0.7rem] text-muted-foreground">
						{$t('login.secretHelp')}
					</p>
					<Button type="submit" disabled={submitting} class="mt-1">
						{submitting ? $t('login.unlocking') : $t('login.unlockSession')}
					</Button>
					{#if toast}
						<div
							class={cn(
								'rounded-md px-3 py-2 text-xs',
								toast.kind === 'ok'
									? 'bg-[hsl(var(--ok)/0.15)] text-[hsl(var(--ok))]'
									: 'bg-[hsl(var(--err)/0.15)] text-[hsl(var(--err))]'
							)}
						>
							{toast.msg}
						</div>
					{/if}
				</form>
			{/if}
		</div>
		<p class="mt-3 text-center text-[0.7rem] text-muted-foreground">
			{$t('login.footer', { version })}
		</p>
	</div>
</div>
