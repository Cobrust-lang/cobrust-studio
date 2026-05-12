<!--
	Index page — redirects to /adr (the default landing after login).
	Done in the browser via goto() because adapter-static + ssr=false
	rules out a server-side redirect.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { t } from '$lib/i18n';
	import { onMount } from 'svelte';

	onMount(() => {
		goto('/adr', { replaceState: true });
	});
</script>

<div class="grid min-h-[60vh] place-items-center text-sm text-muted-foreground">
	{$t('common.loading')}
</div>
