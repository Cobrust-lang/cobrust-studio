<!--
	Status/severity badge — caller supplies the class via `class` so
	$lib/util.adrStatusClass / severityClass / findingStatusClass drive
	the colour.
-->
<script lang="ts">
	import { cn } from '$lib/util';
	import type { Snippet } from 'svelte';

	interface Props {
		class?: string;
		children: Snippet;
	}
	let { class: klass = '', children }: Props = $props();
</script>

<span
	class={cn(
		'inline-flex items-center rounded-full px-2 py-0.5 text-[0.7rem] font-mono font-medium uppercase tracking-wide',
		klass
	)}
>
	{@render children()}
</span>
