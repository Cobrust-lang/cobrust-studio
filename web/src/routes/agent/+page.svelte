<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { agentTurnSse, getSessionModels, getSessionStatus, ApiError } from '$lib/api';
	import Button from '$lib/components/Button.svelte';
	import { t } from '$lib/i18n';
	import { cn } from '$lib/util';
	import type {
		AgentTurnDone,
		AgentTurnIteration,
		AgentTurnToolCall,
		AgentTurnToolResult
	} from '$lib/types';

	type TimelineEntry =
		| { kind: 'iteration'; payload: AgentTurnIteration }
		| { kind: 'tool_call'; payload: AgentTurnToolCall }
		| { kind: 'tool_result'; payload: AgentTurnToolResult }
		| { kind: 'done'; payload: AgentTurnDone }
		| { kind: 'error'; payload: { error: string; code: string } };

	const readOnlyTools = ['fs.read', 'fs.list', 'git.status', 'git.diff', 'project_tree'];

	let model = $state('');
	let modelOptions = $state<string[]>([]);
	let loadingModels = $state(true);
	let temperature = $state(0.2);
	let taskTag = $state('agent-turn');
	let prompt = $state('Summarise the current ADR status in one sentence.');
	let systemPrompt = $state('You are a calm, precise assistant. Use tools to investigate.');

	let streaming = $state(false);
	let timeline = $state<TimelineEntry[]>([]);
	let done = $state<AgentTurnDone | null>(null);
	let err = $state<{ code: string; message: string } | null>(null);
	let routerMissing = $state(false);

	let abortCtrl: AbortController | null = null;

	onMount(async () => {
		loadingModels = true;
		try {
			const status = await getSessionStatus();
			if (!status.authenticated) {
				routerMissing = true;
				return;
			}
			model = status.selected_model ?? '';
			try {
				const sessionModels = await getSessionModels();
				modelOptions = sessionModels.models;
				model = sessionModels.selected_model ?? status.selected_model ?? sessionModels.models[0] ?? '';
			} catch (e) {
				if (!model) {
					throw e;
				}
				const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
				err = { code: 'model_list_failed', message: msg };
			}
		} catch (e) {
			if (e instanceof ApiError) {
				err = { code: e.code, message: e.message };
				routerMissing = e.code === 'no_session' || e.code === 'router_not_configured';
			} else {
				err = { code: 'transport_error', message: String(e) };
			}
		} finally {
			loadingModels = false;
		}
	});

	async function start(e: SubmitEvent) {
		e.preventDefault();
		if (streaming) return;
		timeline = [];
		done = null;
		err = null;
		routerMissing = false;
		if (!model.trim()) {
			err = { code: 'invalid_body', message: $t('agent.errModelRequired') };
			return;
		}
		if (!prompt.trim()) {
			err = { code: 'invalid_body', message: $t('agent.errPromptRequired') };
			return;
		}
		streaming = true;
		abortCtrl = new AbortController();
		try {
			const gen = agentTurnSse(
				{
					model: model.trim(),
					system: systemPrompt.trim() || undefined,
					messages: [{ role: 'user', content: prompt.trim() }],
					params: { temperature: Number(temperature) },
					max_iterations: 16,
					tools_allowed: readOnlyTools,
					task_tag: taskTag.trim() || undefined
				},
				abortCtrl.signal
			);
			for await (const evt of gen) {
				timeline = [...timeline, evt];
				if (evt.kind === 'done') {
					done = evt.payload;
				} else if (evt.kind === 'error') {
					err = { code: evt.payload.code, message: evt.payload.error };
				}
			}
		} catch (e) {
			if ((e as Error).name === 'AbortError') return;
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

	function iterationCount() {
		return done?.iterations ?? timeline.filter((entry) => entry.kind === 'iteration').length;
	}

	function toolCallCount() {
		return timeline.filter((entry) => entry.kind === 'tool_call').length;
	}

	function formatValue(value: unknown): string {
		if (typeof value === 'string') return value;
		try {
			return JSON.stringify(value, null, 2);
		} catch {
			return String(value);
		}
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

<div class="grid gap-5 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
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
					list="agent-model-options"
					placeholder={loadingModels ? $t('common.loading') : $t('agent.modelPlaceholder')}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
				<datalist id="agent-model-options">
					{#each modelOptions as option}
						<option value={option}></option>
					{/each}
				</datalist>
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

		<div class="rounded-md bg-secondary/30 px-3 py-2 text-xs text-muted-foreground">
			{$t('agent.toolsAllowed', { tools: readOnlyTools.join(', ') })}
		</div>

		<div class="flex gap-2 pt-1">
			<Button type="submit" disabled={streaming || loadingModels}>
				{streaming ? $t('agent.streaming') : $t('agent.run')}
			</Button>
			{#if streaming}
				<Button type="button" variant="ghost" onclick={cancel}>{$t('common.cancel')}</Button>
			{/if}
		</div>
	</form>

	<div class="flex min-h-[24rem] flex-col gap-3 rounded-lg border border-border bg-card p-4">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<h2 class="text-sm font-semibold">{$t('agent.timeline')}</h2>
			<div class="flex items-center gap-2 text-[0.7rem] text-muted-foreground">
				<span class="font-mono">{$t('agent.iterations', { count: iterationCount() })}</span>
				<span class="font-mono">{$t('agent.toolCalls', { count: toolCallCount() })}</span>
				{#if done}
					<span class="font-mono">
						{$t('agent.tokens', {
							prompt: done.total_tokens.prompt_tokens,
							completion: done.total_tokens.completion_tokens
						})}
					</span>
					{#if done.task_tag}
						<span class="font-mono">{$t('agent.tag', { tag: done.task_tag })}</span>
					{/if}
				{/if}
			</div>
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

		<div class="flex-1 space-y-3 overflow-auto rounded-md bg-secondary/20 p-3">
			{#if timeline.length === 0}
				<div class="rounded-md border border-dashed border-border px-3 py-8 text-center text-sm text-muted-foreground">
					{streaming ? $t('agent.awaitingFirstEvent') : $t('agent.runToPopulate')}
				</div>
			{:else}
				{#each timeline as entry}
					{#if entry.kind === 'iteration'}
						<section class="rounded-md border border-border bg-card p-3">
							<div class="flex flex-wrap items-center justify-between gap-2 text-xs">
								<div class="font-semibold">
									{$t('agent.iterationLabel', { n: entry.payload.n + 1 })}
									<span class="font-normal text-muted-foreground">· {entry.payload.stop_reason}</span>
								</div>
								<div class="flex flex-wrap gap-2 text-[0.7rem] text-muted-foreground">
									<span class="font-mono">{entry.payload.model}</span>
									<span class="font-mono">
										{$t('agent.tokens', {
											prompt: entry.payload.usage.prompt_tokens,
											completion: entry.payload.usage.completion_tokens
										})}
									</span>
									{#if entry.payload.cache_hit}
										<span class="rounded-full bg-[hsl(var(--ok)/0.18)] px-2 py-0.5 text-[hsl(var(--ok))]"
											>{$t('agent.cacheHit')}</span
										>
									{/if}
								</div>
							</div>
							<pre class="mt-2 whitespace-pre-wrap break-words rounded bg-secondary/30 p-2 font-mono text-xs leading-relaxed">{entry.payload.text}</pre>
						</section>
					{:else if entry.kind === 'tool_call'}
						<section class="ml-4 rounded-md border border-border bg-background p-3">
							<div class="text-xs font-semibold">
								{$t('agent.toolCallLabel')}
								<span class="font-mono text-muted-foreground">{entry.payload.tool}</span>
							</div>
							<pre class="mt-2 whitespace-pre-wrap break-words rounded bg-secondary/30 p-2 font-mono text-xs leading-relaxed">{formatValue(entry.payload.input)}</pre>
						</section>
					{:else if entry.kind === 'tool_result'}
						<section
							class={cn(
								'ml-4 rounded-md border p-3',
								entry.payload.error
									? 'border-[hsl(var(--err)/0.3)] bg-[hsl(var(--err)/0.08)]'
									: 'border-[hsl(var(--ok)/0.25)] bg-[hsl(var(--ok)/0.08)]'
							)}
						>
							<div class="flex flex-wrap items-center justify-between gap-2 text-xs font-semibold">
								<div>
									{$t('agent.toolResultLabel')}
									<span class="font-mono text-muted-foreground">{entry.payload.tool}</span>
								</div>
								<span class="font-mono text-[0.7rem] text-muted-foreground">{entry.payload.ms}ms</span>
							</div>
							{#if entry.payload.error}
								<div class="mt-2 rounded bg-[hsl(var(--err)/0.12)] p-2 text-xs text-[hsl(var(--err))]">
									<div class="font-mono uppercase opacity-70">{entry.payload.error.code}</div>
									<div>{entry.payload.error.message}</div>
								</div>
							{:else}
								<pre class="mt-2 whitespace-pre-wrap break-words rounded bg-background/70 p-2 font-mono text-xs leading-relaxed">{formatValue(entry.payload.output)}</pre>
							{/if}
						</section>
					{:else if entry.kind === 'done'}
						<section class="rounded-md border border-primary/30 bg-primary/5 p-3">
							<div class="text-xs font-semibold">{$t('agent.finalText')}</div>
							<pre class="mt-2 whitespace-pre-wrap break-words rounded bg-background/70 p-2 font-mono text-xs leading-relaxed">{entry.payload.final_text}</pre>
						</section>
					{:else if entry.kind === 'error'}
						<section class="rounded-md border border-[hsl(var(--err)/0.3)] bg-[hsl(var(--err)/0.08)] p-3 text-sm text-[hsl(var(--err))]">
							<div class="font-mono text-[0.7rem] uppercase tracking-wide opacity-70">{entry.payload.code}</div>
							<div>{entry.payload.error}</div>
						</section>
					{/if}
				{/each}
			{/if}
			{#if streaming && timeline.length > 0}
				<div class="text-xs text-muted-foreground">{$t('agent.awaitingNextEvent')}</div>
			{/if}
		</div>
	</div>
</div>
