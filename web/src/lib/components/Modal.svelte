<!--
	Minimal modal dialog primitive — uses the native <dialog> element so
	we get focus trap + ESC handling for free. The HTML `<dialog>` API
	on Chromium/Firefox/Safari is now stable (mid-2025); no polyfill
	needed for the supported browser matrix.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		open: boolean;
		onclose?: () => void;
		title?: string;
		children: Snippet;
		/** Optional footer slot (e.g. action buttons). */
		footer?: Snippet;
		/** Width preset; defaults to `md` ~ max-w-2xl. */
		width?: 'sm' | 'md' | 'lg';
	}
	let { open = $bindable(), onclose, title, children, footer, width = 'md' }: Props = $props();

	let dialogEl: HTMLDialogElement | undefined = $state();

	$effect(() => {
		if (!dialogEl) return;
		if (open && !dialogEl.open) dialogEl.showModal();
		else if (!open && dialogEl.open) dialogEl.close();
	});

	const widthCls = $derived(
		{
			sm: 'max-w-md',
			md: 'max-w-2xl',
			lg: 'max-w-4xl'
		}[width]
	);

	function close() {
		open = false;
		onclose?.();
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			close();
		}
	}
</script>

<dialog
	bind:this={dialogEl}
	{onkeydown}
	onclose={close}
	class={`w-full ${widthCls} rounded-lg bg-card text-card-foreground p-0 backdrop:bg-black/60`}
>
	{#if open}
		<div class="flex flex-col max-h-[80vh]">
			{#if title}
				<div class="flex items-center justify-between border-b border-border px-5 py-3">
					<h2 class="text-base font-semibold">{title}</h2>
					<button
						type="button"
						onclick={close}
						class="text-muted-foreground hover:text-foreground text-lg leading-none"
						aria-label="Close"
					>
						×
					</button>
				</div>
			{/if}
			<div class="overflow-auto px-5 py-4 text-sm">{@render children()}</div>
			{#if footer}
				<div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
					{@render footer()}
				</div>
			{/if}
		</div>
	{/if}
</dialog>

<style>
	/* Center the dialog on the viewport; native showModal() handles backdrop. */
	dialog {
		margin: auto;
		border: 1px solid hsl(var(--border));
		box-shadow:
			0 10px 30px rgba(0, 0, 0, 0.35),
			0 2px 8px rgba(0, 0, 0, 0.2);
	}
</style>
