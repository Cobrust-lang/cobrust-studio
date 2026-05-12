<!--
	/agent — dispatch composer + SSE live stream.

	Wire surface:
	- POST /api/dispatch → SSE: `event: chunk` (delta) → `event: done` (summary)
	  or `event: error` (envelope).
	- 503 router_not_configured → "Configure LLM endpoint" CTA → /login.
-->
<script lang="ts">
	import { onDestroy } from 'svelte';
	import { dispatchSse, ApiError } from '$lib/api';
	import Button from '$lib/components/Button.svelte';
	import { t } from '$lib/i18n';
	import { cn } from '$lib/util';

	let model = $state('claude-opus-4-7');
	let temperature = $state(0.2);
	let taskTag = $state('agent-turn');
	let prompt = $state('Summarise the current ADR status in one sentence.');
	let systemPrompt = $state('You are a calm, precise assistant.');

	let streaming = $state(false);
	let transcript = $state('');
	let done = $state<{
		provider: string;
		model: string;
		usage: { prompt_tokens: number; completion_tokens: number };
		cache_hit: boolean;
		task_tag: string | null;
	} | null>(null);
	let err = $state<{ code: string; message: string } | null>(null);
	let routerMissing = $state(false);

	let abortCtrl: AbortController | null = null;

	async function start(e: SubmitEvent) {
		e.preventDefault();
		if (streaming) return;
		transcript = '';
		done = null;
		err = null;
		routerMissing = false;
		if (!prompt.trim()) {
			err = { code: 'invalid_body', message: $t('agent.errPromptRequired') };
			return;
		}
		streaming = true;
		abortCtrl = new AbortController();
		try {
			const messages = [];
			if (systemPrompt.trim())
				messages.push({ role: 'system' as const, content: systemPrompt.trim() });
			messages.push({ role: 'user' as const, content: prompt });
			const gen = dispatchSse(
				{
					model: model.trim(),
					messages,
					params: { temperature: Number(temperature) },
					task_tag: taskTag.trim() || undefined
				},
				abortCtrl.signal
			);
			for await (const evt of gen) {
				if (evt.kind === 'chunk') {
					transcript += evt.delta;
				} else if (evt.kind === 'done') {
					done = evt.payload;
					if (transcript === '' && evt.payload.text) transcript = evt.payload.text;
				} else if (evt.kind === 'error') {
					err = { code: evt.payload.code, message: evt.payload.error };
				}
			}
		} catch (e) {
			if (e instanceof ApiError) {
				err = { code: e.code, message: e.message };
				routerMissing = e.code === 'router_not_configured';
			} else {
				err = { code: 'transport_error', message: String(e) };
			}
		} finally {
			streaming = false;
			abortCtrl = null;
		}
	}

	function cancel() {
		abortCtrl?.abort();
	}

	onDestroy(() => {
		abortCtrl?.abort();
	});
</script>

<header class="mb-5">
	<h1 class="text-lg font-semibold">{$t('agent.title')}</h1>
	<p class="text-xs text-muted-foreground">
		{$t('agent.subtitle')}
	</p>
</header>

<div class="grid gap-5 lg:grid-cols-2">
	<form onsubmit={start} class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
		<h2 class="text-sm font-semibold">{$t('agent.prompt')}</h2>

		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">{$t('agent.system')}</span>
			<textarea
				bind:value={systemPrompt}
				rows="2"
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono resize-y"
			></textarea>
		</label>

		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">{$t('agent.user')}</span>
			<textarea
				bind:value={prompt}
				rows="6"
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono resize-y"
			></textarea>
		</label>

		<div class="grid grid-cols-2 gap-3">
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">{$t('common.model')}</span>
				<input
					type="text"
					bind:value={model}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">{$t('agent.taskTag')}</span>
				<input
					type="text"
					bind:value={taskTag}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
			</label>
		</div>

		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground"
				>{$t('agent.temperature', { value: temperature.toFixed(2) })}</span
			>
			<input
				type="range"
				min="0"
				max="1"
				step="0.05"
				bind:value={temperature}
				class="accent-primary"
			/>
		</label>

		<div class="flex gap-2 pt-1">
			<Button type="submit" disabled={streaming}>
				{streaming ? $t('agent.streaming') : $t('agent.dispatch')}
			</Button>
			{#if streaming}
				<Button type="button" variant="ghost" onclick={cancel}>{$t('common.cancel')}</Button>
			{/if}
		</div>
	</form>

	<div class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4 min-h-[24rem]">
		<div class="flex items-center justify-between">
			<h2 class="text-sm font-semibold">{$t('agent.output')}</h2>
			{#if done}
				<div class="flex items-center gap-2 text-[0.7rem] text-muted-foreground">
					<span class="font-mono">{done.provider}/{done.model}</span>
					<span class="font-mono">
						{$t('agent.tokens', {
							prompt: done.usage.prompt_tokens,
							completion: done.usage.completion_tokens
						})}
					</span>
					{#if done.cache_hit}
						<span class="rounded-full bg-[hsl(var(--ok)/0.18)] px-2 py-0.5 text-[hsl(var(--ok))]"
							>{$t('agent.cacheHit')}</span
						>
					{/if}
					{#if done.task_tag}
						<span class="font-mono">{$t('agent.tag', { tag: done.task_tag })}</span>
					{/if}
				</div>
			{/if}
		</div>

		{#if routerMissing}
			<div class="rounded-md bg-[hsl(var(--warn)/0.12)] px-3 py-2 text-sm text-[hsl(var(--warn))]">
				{$t('agent.routerMissing')}
				<a href="/login" class="underline">{$t('agent.configureEndpoint')}</a>
			</div>
		{:else if err}
			<div class="rounded-md bg-[hsl(var(--err)/0.12)] px-3 py-2 text-sm text-[hsl(var(--err))]">
				<div class="font-mono text-[0.7rem] uppercase tracking-wide opacity-70">{err.code}</div>
				<div>{err.message}</div>
			</div>
		{/if}

		<pre
			class={cn(
				'flex-1 whitespace-pre-wrap break-words rounded-md bg-secondary/30 p-3 font-mono text-xs leading-relaxed',
				transcript === '' && 'text-muted-foreground/60'
			)}>{transcript ||
				(streaming ? $t('agent.awaitingFirstChunk') : $t('agent.dispatchToPopulate'))}</pre>
	</div>
</div>
