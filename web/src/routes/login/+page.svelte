<!--
	/login — custom endpoint + API key per ADR-0003.

	Two tabs visually: "API key" (active, drives the M2 flow) and "OAuth"
	(greyed out with a "Coming v0.5.0" placeholder; clicking does
	nothing). The API key is client-side AES-GCM-encrypted via the M2
	stub in `$lib/crypto.ts` before POSTing to `/api/auth/set-endpoint`.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import Button from '$lib/components/Button.svelte';
	import { setEndpoint, ApiError } from '$lib/api';
	import { encryptEndpointBlob } from '$lib/crypto';
	import { cn } from '$lib/util';

	let tab = $state<'api' | 'oauth'>('api');
	let baseUrl = $state('https://api.anthropic.com');
	let apiKey = $state('');
	let model = $state('claude-opus-4-7');
	let submitting = $state(false);
	let toast = $state<{ kind: 'ok' | 'err'; msg: string } | null>(null);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		toast = null;
		if (!baseUrl.trim() || !apiKey.trim() || !model.trim()) {
			toast = { kind: 'err', msg: 'all fields required' };
			return;
		}
		submitting = true;
		try {
			const blob = await encryptEndpointBlob({
				base_url: baseUrl.trim(),
				api_key: apiKey.trim(),
				model: model.trim()
			});
			await setEndpoint(blob);
			toast = { kind: 'ok', msg: 'endpoint stored — redirecting…' };
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
					<p class="text-[0.7rem] text-muted-foreground">
						Encrypted client-side (AES-GCM-256, M2 stub) before transit. Real AEAD scheme lands at
						M3.
					</p>
					<Button type="submit" disabled={submitting} class="mt-1">
						{submitting ? 'Storing…' : 'Save endpoint'}
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
			Studio is a single-binary, single-project, single-user MVP. v0.1.0.
		</p>
	</div>
</div>
