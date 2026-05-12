<!--
	/finding — Finding list + detail + create.

	Wire surface:
	- GET /api/finding   → table rows
	- POST /api/finding  → create dialog
	- GET /api/events    → live re-list on `finding_added` / modified / removed

	Note: the M1 server contract does not expose `GET /api/finding/:id`
	(see `crates/studio-server/src/routes/finding.rs` doc comment). The
	detail dialog shows the summary fields plus the row's path link;
	full-body view arrives in M2+ when the server grows a singleton route.
-->
<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import Badge from '$lib/components/Badge.svelte';
	import Button from '$lib/components/Button.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { ApiError, createFinding, listFindings, subscribeEvents } from '$lib/api';
	import type { FindingSummary } from '$lib/types';
	import { cn, findingStatusClass, severityClass } from '$lib/util';

	let rows = $state<FindingSummary[]>([]);
	let loadErr = $state<string | null>(null);
	let loading = $state(true);

	let detailOpen = $state(false);
	let detailRow = $state<FindingSummary | null>(null);

	let createOpen = $state(false);
	let createBusy = $state(false);
	let createErr = $state<string | null>(null);

	let formId = $state('');
	let formTitle = $state('');
	let formSeverity = $state('P3');
	let formStatus = $state('open');
	let formVerified = $state('HEAD');
	let formDeps = $state('');
	let formRelated = $state('');
	let formBody = $state('');

	let unsubEvents: (() => void) | null = null;

	async function refresh() {
		loading = true;
		loadErr = null;
		try {
			rows = await listFindings();
		} catch (e) {
			loadErr = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
		} finally {
			loading = false;
		}
	}

	function openDetail(row: FindingSummary) {
		detailRow = row;
		detailOpen = true;
	}

	function openCreate() {
		formId = '';
		formTitle = '';
		formSeverity = 'P3';
		formStatus = 'open';
		formVerified = 'HEAD';
		formDeps = '';
		formRelated = '';
		formBody = '';
		createErr = null;
		createOpen = true;
	}

	async function submitCreate(e: SubmitEvent) {
		e.preventDefault();
		createErr = null;
		if (!formId.trim() || !formTitle.trim()) {
			createErr = 'finding_id and title are required';
			return;
		}
		const splitList = (s: string) =>
			s
				.split(',')
				.map((x) => x.trim())
				.filter(Boolean);
		createBusy = true;
		try {
			await createFinding({
				finding_id: formId.trim(),
				title: formTitle.trim(),
				severity: formSeverity,
				status: formStatus,
				last_verified_commit: formVerified.trim() || 'HEAD',
				dependencies: splitList(formDeps),
				related: splitList(formRelated),
				body: formBody
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
			if (
				evt.kind === 'finding_added' ||
				evt.kind === 'finding_modified' ||
				evt.kind === 'finding_removed'
			) {
				refresh();
			}
		});
	});

	onDestroy(() => unsubEvents?.());
</script>

<header class="mb-5 flex items-end justify-between gap-4">
	<div>
		<h1 class="text-lg font-semibold">Findings</h1>
		<p class="text-xs text-muted-foreground">
			Failure-capture log — severity P0..P3, status open / closed_by_*. {rows.length} entries.
		</p>
	</div>
	<Button onclick={openCreate}>+ New finding</Button>
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
				<th class="px-3 py-2 text-left">ID</th>
				<th class="px-3 py-2 text-left">Title</th>
				<th class="px-3 py-2 text-left w-20">Severity</th>
				<th class="px-3 py-2 text-left w-32">Status</th>
				<th class="px-3 py-2 text-left">Verified commit</th>
			</tr>
		</thead>
		<tbody>
			{#if loading}
				<tr><td colspan="5" class="px-3 py-6 text-center text-muted-foreground">Loading…</td></tr>
			{:else if rows.length === 0}
				<tr
					><td colspan="5" class="px-3 py-6 text-center text-muted-foreground">No findings yet.</td
					></tr
				>
			{/if}
			{#each rows as f (f.finding_id)}
				<tr
					class="cursor-pointer border-t border-border hover:bg-secondary/30"
					onclick={() => openDetail(f)}
				>
					<td class="px-3 py-2 font-mono text-xs text-muted-foreground">{f.finding_id}</td>
					<td class="px-3 py-2">{f.title}</td>
					<td class="px-3 py-2">
						<Badge class={severityClass(f.severity)}>{f.severity}</Badge>
					</td>
					<td class="px-3 py-2">
						<Badge class={findingStatusClass(f.status)}>{f.status}</Badge>
					</td>
					<td class="px-3 py-2 font-mono text-xs text-muted-foreground">{f.date}</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>

<!-- Detail dialog -->
<Modal bind:open={detailOpen} title={detailRow ? detailRow.finding_id : 'Finding'} width="md">
	{#if detailRow}
		<div class="flex flex-col gap-3 text-sm">
			<div class="flex flex-wrap items-center gap-2 text-xs">
				<Badge class={severityClass(detailRow.severity)}>{detailRow.severity}</Badge>
				<Badge class={findingStatusClass(detailRow.status)}>{detailRow.status}</Badge>
				<span class="font-mono text-muted-foreground">{detailRow.date}</span>
			</div>
			<div>
				<h3 class="text-sm font-semibold">{detailRow.title}</h3>
			</div>
			<p class="text-[0.7rem] text-muted-foreground break-all" title={detailRow.path}>
				{detailRow.path}
			</p>
			<p class="text-xs text-muted-foreground">
				Body view requires a singleton <code>/api/finding/:id</code> route — planned for M2+. Open the
				file on disk to inspect the full markdown.
			</p>
		</div>
	{/if}
</Modal>

<!-- Create dialog -->
<Modal bind:open={createOpen} title="New finding" width="md">
	<form onsubmit={submitCreate} class="flex flex-col gap-3">
		<div class="grid grid-cols-2 gap-3">
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">finding_id</span>
				<input
					type="text"
					bind:value={formId}
					required
					placeholder="m2-frontend-tab-leak"
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">Severity</span>
				<select
					bind:value={formSeverity}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none"
				>
					<option value="P0">P0</option>
					<option value="P1">P1</option>
					<option value="P2">P2</option>
					<option value="P3">P3</option>
				</select>
			</label>
		</div>
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
					<option value="open">open</option>
					<option value="closed_by_a1.1">closed_by_a1.1</option>
					<option value="closed_by_a2.1">closed_by_a2.1</option>
					<option value="closed_by_m2">closed_by_m2</option>
				</select>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				<span class="text-xs text-muted-foreground">last_verified_commit</span>
				<input
					type="text"
					bind:value={formVerified}
					class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
				/>
			</label>
		</div>
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Dependencies (comma-separated)</span>
			<input
				type="text"
				bind:value={formDeps}
				placeholder="adr:0006, finding:a1-1-strip"
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
			/>
		</label>
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Related (comma-separated)</span>
			<input
				type="text"
				bind:value={formRelated}
				class="rounded-md border border-input bg-background px-3 py-1.5 text-sm focus:border-ring focus:outline-none font-mono"
			/>
		</label>
		<label class="flex flex-col gap-1 text-sm">
			<span class="text-xs text-muted-foreground">Body (markdown)</span>
			<textarea
				bind:value={formBody}
				rows="6"
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
				{createBusy ? 'Creating…' : 'Create finding'}
			</Button>
		</div>
	</form>
</Modal>
