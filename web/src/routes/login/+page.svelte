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
	import { login, getVersion, ApiError } from '$lib/api';
	import { cn } from '$lib/util';

	let tab = $state<'api' | 'oauth'>('api');
	let baseUrl = $state('https://api.anthropic.com');
	let apiKey = $state('');
	let model = $state('claude-opus-4-7');
	let passphrase = $state('');
	let providerKind = $state<'anthropic' | 'openai'>('anthropic');
	let submitting = $state(false);
	let toast = $state<{ kind: 'ok' | 'err'; msg: string } | null>(null);

	// Mei v3 P2: footer used to hardcode "v0.1.0" which drifted with every
	// release. Fetch from /api/version on mount so the footer always
	// matches the binary the user is talking to.
	let version = $state<string>('…');
	onMount(async () => {
		try {
			const v = await getVersion();
			version = v.studio_server;
		} catch {
			version = 'unknown';
		}
	});

	// M7 (ADR-0008): URL-based auto-suggest for provider kind.
	// Reactive: typing the URL updates the dropdown, but the user can override.
	$effect(() => {
		if (baseUrl.includes('anthropic.com')) {
			providerKind = 'anthropic';
		} else if (baseUrl.length > 0 && !baseUrl.includes('anthropic.com')) {
			providerKind = 'openai';
		}
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		toast = null;
		if (!baseUrl.trim() || !apiKey.trim() || !model.trim() || !passphrase) {
			toast = { kind: 'err', msg: 'all fields required (including passphrase)' };
			return;
		}
		if (passphrase.length < 8) {
			toast = { kind: 'err', msg: 'passphrase must be ≥ 8 characters' };
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
			toast = { kind: 'ok', msg: 'session unlocked — redirecting…' };
			setTimeout(() => goto('/adr'), 400);
		} catch (e) {
			const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
			toast = { kind: 'err', msg };
		} finally {
			submitting = false;
		}
	}
</script>

<div class="grid min-h-screen place-items-center px-4">
	<div class="w-full max-w-md">
		<div class="mb-6 text-center">
			<div
				class="mx-auto mb-3 grid h-10 w-10 place-items-center rounded-lg bg-primary text-primary-foreground font-mono text-sm font-bold"
			>
				CS
			</div>
			<h1 class="text-xl font-semibold">Cobrust Studio</h1>
			<p class="mt-1 text-xs text-muted-foreground">
				Configure your LLM endpoint to start dispatching agents.
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
					API key
				</button>
				<button
					type="button"
					disabled
					class="flex-1 rounded px-2 py-1.5 text-muted-foreground/60 cursor-not-allowed"
					title="OAuth lands in v0.5.0"
				>
					OAuth <span class="ml-1 text-[0.65rem] opacity-70">(v0.5.0)</span>
				</button>
			</div>

			{#if tab === 'api'}
				<form onsubmit={submit} class="flex flex-col gap-3">
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">Base URL</span>
						<input
							type="text"
							bind:value={baseUrl}
							placeholder="https://api.anthropic.com"
							autocomplete="off"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
						/>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">API key</span>
						<input
							type="password"
							bind:value={apiKey}
							autocomplete="off"
							placeholder="sk-…"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">Model</span>
						<input
							type="text"
							bind:value={model}
							placeholder="claude-opus-4-7"
							autocomplete="off"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">Provider</span>
						<select
							bind:value={providerKind}
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
						>
							<option value="anthropic">Anthropic API</option>
							<option value="openai"
								>OpenAI-compatible (vLLM / DeepSeek / Together / OpenRouter / Groq /
								Ollama)</option
							>
						</select>
					</label>
					<label class="flex flex-col gap-1 text-sm">
						<span class="text-xs text-muted-foreground">Passphrase (≥ 8 chars)</span>
						<input
							type="password"
							bind:value={passphrase}
							autocomplete="new-password"
							placeholder="encrypts your API key at rest"
							class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
						/>
					</label>
					<p class="text-[0.7rem] text-muted-foreground">
						Server derives an Argon2id key from your passphrase, AES-256-GCM-seals
						the credentials, and holds the in-memory session key for the binary's
						lifetime (ADR-0007). On restart, re-enter the passphrase. The
						plaintext travels over TLS / localhost; the server never persists it.
					</p>
					<Button type="submit" disabled={submitting} class="mt-1">
						{submitting ? 'Unlocking…' : 'Unlock session'}
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
			Studio is a single-binary, single-project, single-user MVP. v{version}.
		</p>
	</div>
</div>
