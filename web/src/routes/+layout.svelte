<!--
	Root layout — navbar (logo, page links, status indicator, theme
	toggle) wraps every page. Theme toggle flips the `.dark` /
	`.light` class on <html> (no SSR involved; adapter-static + ssr=false
	means the toggle is a pure browser concern).
-->
<script lang="ts">
	import '../app.css';
	import { page } from '$app/stores';
	import { appState } from '$lib/store.svelte';
	import { cn } from '$lib/util';
	import type { Snippet } from 'svelte';
	import { onMount } from 'svelte';

	interface Props {
		children: Snippet;
	}
	let { children }: Props = $props();

	let theme = $state<'dark' | 'light'>('dark');

	onMount(() => {
		const saved = localStorage.getItem('cs-theme');
		if (saved === 'light' || saved === 'dark') {
			theme = saved;
			applyTheme(theme);
		}
	});

	function applyTheme(t: 'dark' | 'light') {
		const html = document.documentElement;
		html.classList.remove('dark', 'light');
		html.classList.add(t);
	}

	function toggleTheme() {
		theme = theme === 'dark' ? 'light' : 'dark';
		localStorage.setItem('cs-theme', theme);
		applyTheme(theme);
	}

	const navItems = [
		{ href: '/adr', label: 'ADRs' },
		{ href: '/finding', label: 'Findings' },
		{ href: '/agent', label: 'Agent' },
		{ href: '/ledger', label: 'Ledger' }
	];

	let currentPath = $derived($page.url.pathname);
	let isLogin = $derived(currentPath === '/login' || currentPath === '/');
</script>

<div class="min-h-screen flex flex-col">
	{#if !isLogin}
		<header class="border-b border-border bg-card/40 backdrop-blur">
			<nav class="mx-auto flex max-w-6xl items-center justify-between gap-4 px-5 py-2.5">
				<a href="/adr" class="flex items-center gap-2 text-sm font-semibold">
					<span
						class="grid h-7 w-7 place-items-center rounded-md bg-primary text-primary-foreground font-mono text-xs"
						>CS</span
					>
					<span class="hidden sm:inline">Cobrust Studio</span>
				</a>
				<div class="flex items-center gap-1">
					{#each navItems as item}
						<a
							href={item.href}
							class={cn(
								'rounded-md px-2.5 py-1 text-sm transition-colors',
								currentPath.startsWith(item.href)
									? 'bg-secondary text-foreground'
									: 'text-muted-foreground hover:text-foreground hover:bg-secondary/60'
							)}
						>
							{item.label}
						</a>
					{/each}
				</div>
				<div class="flex items-center gap-3 text-xs">
					{#if appState.project}
						<span
							class="hidden md:inline text-muted-foreground"
							title={appState.project.project_root}
						>
							<span class="text-foreground/80 font-mono">
								{appState.project.project_root.split('/').slice(-2).join('/')}
							</span>
						</span>
					{/if}
					{#if appState.version}
						<span class="text-muted-foreground font-mono">
							v{appState.version.studio_server}
						</span>
					{/if}
					<span
						class={cn(
							'inline-flex h-2 w-2 rounded-full',
							appState.project ? 'bg-[hsl(var(--ok))]' : 'bg-[hsl(var(--err))]'
						)}
						title={appState.project ? 'Server reachable' : 'Server unreachable'}
					></span>
					<button
						type="button"
						onclick={toggleTheme}
						class="rounded-md px-2 py-1 text-xs text-muted-foreground hover:text-foreground hover:bg-secondary/60"
						aria-label="Toggle theme"
						title="Toggle theme"
					>
						{theme === 'dark' ? '☾' : '☀'}
					</button>
					<a
						href="/login"
						class="rounded-md px-2 py-1 text-xs text-muted-foreground hover:text-foreground hover:bg-secondary/60"
						title="Endpoint settings"
					>
						endpoint
					</a>
				</div>
			</nav>
		</header>
	{/if}
	<main class={cn('flex-1', isLogin ? '' : 'mx-auto w-full max-w-6xl px-5 py-6')}>
		{@render children()}
	</main>
</div>
