<!--
	/adr — ADR list + detail + create.

	Wire surface:
	- GET /api/adr      → table rows
	- GET /api/adr/:id  → detail dialog body
	- POST /api/adr     → create dialog
	- GET /api/events   → live append on `adr_added` / `adr_modified` / `adr_removed`
-->
<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { ApiError, createAdr, getAdr, listAdrs, subscribeEvents } from '$lib/api';
	import type { Adr, AdrSummary } from '$lib/types';
	import { adrStatusClass, cn } from '$lib/util';

	let rows = $state<AdrSummary[]>([]);
	let loadErr = $state<string | null>(null);
	let loading = $state(true);

	let detailOpen = $state(false);
	let detailAdr = $state<Adr | null>(null);
	let detailErr = $state<string | null>(null);

	let createOpen = $state(false);
	let createBusy = $state(false);
	let createErr = $state<string | null>(null);

	// Form state for the create dialog
	let formTitle = $state('');
	let formStatus = $state('proposed');
	let formDate = $state(today());
	let formBody = $state('');
	let formSupersedes = $state('');

	let unsubEvents: (() => void) | null = null;

	function today(): string {
		const d = new Date();
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
	}

	async function refresh() {
		loading = true;
		loadErr = null;
		try {
			const list = await listAdrs();
			// Server contract guarantees adr_id ascending; sort defensively.
			rows = [...list].sort((a, b) => a.adr_id - b.adr_id);
		} catch (e) {
			loadErr = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
		} finally {
			loading = false;
		}
	}

	async function openDetail(id: number) {
		detailAdr = null;
		detailErr = null;
		detailOpen = true;
		try {
			detailAdr = await getAdr(id);
		} catch (e) {
			detailErr = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
		}
	}

	function openCreate() {
		formTitle = '';
		formStatus = 'proposed';
		formDate = today();
		formBody = '';
		formSupersedes = '';
		createErr = null;
		createOpen = true;
	}

	async function submitCreate(e: SubmitEvent) {
		e.preventDefault();
		createErr = null;
		if (!formTitle.trim()) {
			createErr = 'title is required';
			return;
		}
		const supersedes = formSupersedes
			.split(',')
			.map((s) => s.trim())
			.filter(Boolean)
			.map(Number)
			.filter((n) => Number.isFinite(n));
		createBusy = true;
		try {
			await createAdr({
				title: formTitle.trim(),
				status: formStatus.trim() || 'proposed',
				date: formDate.trim() || today(),
				body: formBody,
				supersedes
			});
			createOpen = false;
			await refresh();
		} catch (e) {
			createErr = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
		} finally {
			createBusy = false;
		}
	}

	onMount(() => {
		refresh();
		unsubEvents = subscribeEvents((evt) => {
			if (evt.kind === 'adr_added' || evt.kind === 'adr_modified' || evt.kind === 'adr_removed') {
				// Re-list — cheap, deterministic, no patching state machine.
				refresh();
			}
		});
	});

	onDestroy(() => {
		unsubEvents?.();
	});
</script>

<header class="mb-5 flex items-end justify-between gap-4">
	<div>
		<h1 class="text-lg font-semibold">ADRs</h1>
		<p class="text-xs text-muted-foreground">
			Architecture decision records — frontmatter + markdown body. {rows.length} entries.
		</p>
	</div>
	<Button onclick={openCreate}>+ New ADR</Button>
</header>

{#if loadErr}
	<div class="mb-3 rounded-md bg-[hsl(var(--err)/0.12)] px-3 py-2 text-sm text-[hsl(var(--err))]">
		Failed to load: {loadErr}
	</div>
{/if}

<div class="overflow-hidden rounded-lg border border-border bg-card">
	<table class="w-full text-sm">
		<thead class="bg-secondary/40 text-xs uppercase tracking-wide text-muted-foreground">
			<tr>
				<th class="px-3 py-2 text-left w-20">ID</th>
				<th class="px-3 py-2 text-left">Title</th>
				<th class="px-3 py-2 text-left w-32">Status</th>
				<th class="px-3 py-2 text-left w-28">Date</th>
				<th class="px-3 py-2 text-left">Path</th>
			</tr>
		</thead>
		<tbody>
			{#if loading}
				<tr><td colspan="5" class="px-3 py-6 text-center text-muted-foreground">Loading…</td></tr>
			{:else if rows.length === 0}
				<tr
					><td colspan="5" class="px-3 py-6 text-center text-muted-foreground">No ADRs yet.</td></tr
				>
			{/if}
			{#each rows as adr (adr.adr_id)}
				<tr
					class="cursor-pointer border-t border-border hover:bg-secondary/30"
					onclick={() => openDetail(adr.adr_id)}
				>
					<td class="px-3 py-2 font-mono text-xs">{String(adr.adr_id).padStart(4, '0')}</td>
					<td class="px-3 py-2">{adr.title}</td>
					<td class="px-3 py-2">
						<Badge class={adrStatusClass(adr.status)}>{adr.status}</Badge>
					</td>
					<td class="px-3 py-2 font-mono text-xs text-muted-foreground">{adr.date}</td>
					<td class="px-3 py-2 text-xs text-muted-foreground truncate" title={adr.path}>
						{adr.path.split('/').slice(-3).join('/')}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>

<!-- Detail dialog -->
<Modal
	bind:open={detailOpen}
	title={detailAdr
		? `ADR-${String(detailAdr.adr_id).padStart(4, '0')} — ${detailAdr.title}`
		: 'ADR detail'}
	width="lg"
>
	{#if detailErr}
		<div class="rounded-md bg-[hsl(var(--err)/0.12)] px-3 py-2 text-sm text-[hsl(var(--err))]">
			{detailErr}
		</div>
	{:else if !detailAdr}
		<div class="text-muted-foreground">Loading…</div>
	{:else}
		<div class="mb-3 flex flex-wrap items-center gap-2 text-xs">
			<Badge class={adrStatusClass(detailAdr.status)}>{detailAdr.status}</Badge>
			<span class="font-mono text-muted-foreground">{detailAdr.date}</span>
			{#if detailAdr.supersedes.length > 0}
				<span class="text-muted-foreground"
					>supersedes: {detailAdr.supersedes
						.map((n) => String(n).padStart(4, '0'))
						.join(', ')}</span
				>
			{/if}
			{#if detailAdr.superseded_by.length > 0}
				<span class="text-muted-foreground"
					>superseded by: {detailAdr.superseded_by
						.map((n) => String(n).padStart(4, '0'))
						.join(', ')}</span
				>
			{/if}
		</div>
		<pre
			class="whitespace-pre-wrap break-words rounded-md bg-secondary/40 p-3 font-mono text-xs leading-relaxed">{detailAdr.body}</pre>
		<p class="mt-3 text-[0.7rem] text-muted-foreground truncate" title={detailAdr.path}>
			{detailAdr.path}
		</p>
	{/if}
</Modal>

<!-- Create dialog -->
<Modal bind:open={createOpen} title="New ADR" width="md">
	<form onsubmit={submitCreate} class="flex flex-col gap-3">
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Title</span>
			<input
				type="text"
				bind:value={formTitle}
				required
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
			/>
		</label>
		<div class="grid grid-cols-2 gap-3">
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">Status</span>
				<select
					bind:value={formStatus}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
				>
					<option value="proposed">proposed</option>
					<option value="accepted">accepted</option>
					<option value="superseded">superseded</option>
					<option value="deprecated">deprecated</option>
				</select>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">Date (UTC)</span>
				<input
					type="date"
					bind:value={formDate}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
			</label>
		</div>
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Supersedes (comma-separated IDs)</span>
			<input
				type="text"
				bind:value={formSupersedes}
				placeholder="e.g. 3, 5"
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
			/>
		</label>
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Body (markdown)</span>
			<textarea
				bind:value={formBody}
				rows="8"
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono resize-y"
			></textarea>
		</label>
		{#if createErr}
			<div class="rounded-md bg-[hsl(var(--err)/0.12)] px-3 py-2 text-sm text-[hsl(var(--err))]">
				{createErr}
			</div>
		{/if}
		<div class={cn('flex justify-end gap-2 pt-1')}>
			<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button type="submit" disabled={createBusy}>
				{createBusy ? 'Creating…' : 'Create ADR'}
			</Button>
		</div>
	</form>
</Modal>
