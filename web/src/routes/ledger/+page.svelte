<!--
	/ledger — recent dispatch ledger entries.

	Wire surface:
	- GET /api/ledger/recent[?n=N]  (default 20, max 1000)
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { ApiError, recentLedger } from '$lib/api';
	import Button from '$lib/components/Button.svelte';
	import Badge from '$lib/components/Badge.svelte';
	import { t } from '$lib/i18n';
	import type { LedgerEntry } from '$lib/types';
	import { cn, fmtTs } from '$lib/util';

	let n = $state(20);
	let entries = $state<LedgerEntry[]>([]);
	let loading = $state(true);
	let err = $state<string | null>(null);

	async function refresh() {
		loading = true;
		err = null;
		try {
			entries = await recentLedger(n);
		} catch (e) {
			err = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
		} finally {
			loading = false;
		}
	}

	function outcomeClass(o: LedgerEntry['outcome']): string {
		switch (o) {
			case 'ok':
				return 'bg-[hsl(var(--ok)/0.18)] text-[hsl(var(--ok))]';
			case 'error_transient':
				return 'bg-[hsl(var(--warn)/0.18)] text-[hsl(var(--warn))]';
			case 'error_permanent':
				return 'bg-[hsl(var(--err)/0.18)] text-[hsl(var(--err))]';
			default:
				return 'bg-secondary text-secondary-foreground';
		}
	}

	onMount(refresh);
</script>

<header class="mb-5 flex items-end justify-between gap-4">
	<div>
		<h1 class="text-lg font-semibold">{$t('ledger.title')}</h1>
		<p class="text-xs text-muted-foreground">
			{$t('ledger.subtitle', { count: entries.length })}
		</p>
	</div>
	<div class="flex items-end gap-2">
		<label class="flex flex-col gap-1 text-xs">
			<span class="text-muted-foreground">{$t('ledger.showLast')}</span>
			<input
				type="number"
				min="1"
				max="1000"
				bind:value={n}
				class="w-24 rounded-md border border-input bg-background px-2 py-1 text-sm font-mono focus:border-ring focus:outline-none"
			/>
		</label>
		<Button onclick={refresh}>{$t('common.refresh')}</Button>
	</div>
</header>

{#if err}
	<div class="mb-3 rounded-md bg-[hsl(var(--err)/0.12)] px-3 py-2 text-sm text-[hsl(var(--err))]">
		{$t('common.failedToLoad', { error: err })}
	</div>
{/if}

<div class="overflow-hidden rounded-lg border border-border bg-card">
	<table class="w-full text-xs">
		<thead class="bg-secondary/40 uppercase tracking-wide text-muted-foreground">
			<tr>
				<th class="px-3 py-2 text-left">{$t('ledger.timestamp')}</th>
				<th class="px-3 py-2 text-left">{$t('common.provider')}</th>
				<th class="px-3 py-2 text-left">{$t('common.model')}</th>
				<th class="px-3 py-2 text-left">{$t('ledger.task')}</th>
				<th class="px-3 py-2 text-right">{$t('ledger.cache')}</th>
				<th class="px-3 py-2 text-right">{$t('ledger.tokens')}</th>
				<th class="px-3 py-2 text-right">{$t('ledger.latency')}</th>
				<th class="px-3 py-2 text-left">{$t('ledger.outcome')}</th>
			</tr>
		</thead>
		<tbody>
			{#if loading}
				<tr
					><td colspan="8" class="px-3 py-6 text-center text-muted-foreground"
						>{$t('common.loading')}</td
					></tr
				>
			{:else if entries.length === 0}
				<tr
					><td colspan="8" class="px-3 py-6 text-center text-muted-foreground"
						>{$t('ledger.noRows')}</td
					></tr
				>
			{/if}
			{#each entries as e (e.ts + e.cache_key + e.attempt)}
				<tr class="border-t border-border hover:bg-secondary/30">
					<td class="px-3 py-2 font-mono text-muted-foreground whitespace-nowrap">{fmtTs(e.ts)}</td>
					<td class="px-3 py-2 font-mono">
						{e.provider}
						{#if e.provider_kind}
							<span class="ml-1 text-muted-foreground">({e.provider_kind})</span>
						{/if}
					</td>
					<td class="px-3 py-2 font-mono">{e.model}</td>
					<td class="px-3 py-2 font-mono text-muted-foreground">{e.task_tag ?? '—'}</td>
					<td class="px-3 py-2 text-right">
						{#if e.cache_hit}
							<span class="text-[hsl(var(--ok))]">{$t('ledger.hit')}</span>
						{:else}
							<span class="text-muted-foreground">{$t('ledger.miss')}</span>
						{/if}
					</td>
					<td
						class={cn('px-3 py-2 text-right font-mono', e.attempt > 1 && 'text-[hsl(var(--warn))]')}
					>
						{e.prompt_tokens}/{e.completion_tokens}/{e.total_tokens}
					</td>
					<td class="px-3 py-2 text-right font-mono">{e.latency_ms} ms</td>
					<td class="px-3 py-2">
						<Badge class={outcomeClass(e.outcome)}>{e.outcome}</Badge>
						{#if e.error_code}
							<span class="ml-2 text-muted-foreground">{e.error_code}</span>
						{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
