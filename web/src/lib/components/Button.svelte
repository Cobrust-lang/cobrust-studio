<!--
	Minimal button primitive. We deliberately do not pull shadcn-svelte
	for this surface — a hand-rolled primitive over tailwind tokens is
	smaller and easier to read in a 4-page MVP. Variants mirror
	shadcn-svelte naming so a swap is mechanical if we change our mind
	in M3.
-->
<script lang="ts">
	import { cn } from '$lib/util';
	import type { Snippet } from 'svelte';

	interface Props {
		variant?: 'primary' | 'secondary' | 'ghost' | 'destructive';
		size?: 'sm' | 'md';
		type?: 'button' | 'submit' | 'reset';
		disabled?: boolean;
		class?: string;
		onclick?: (e: MouseEvent) => void;
		children: Snippet;
	}

	let {
		variant = 'primary',
		size = 'md',
		type = 'button',
		disabled = false,
		class: klass = '',
		onclick,
		children
	}: Props = $props();

	const variantCls = {
		primary: 'bg-primary text-primary-foreground hover:opacity-90',
		secondary: 'bg-secondary text-secondary-foreground hover:bg-muted',
		ghost: 'bg-transparent text-foreground hover:bg-secondary',
		destructive: 'bg-destructive text-destructive-foreground hover:opacity-90'
	};
	const sizeCls = {
		sm: 'px-2.5 py-1 text-xs',
		md: 'px-3 py-1.5 text-sm'
	};
</script>

<button
	{type}
	{disabled}
	{onclick}
	class={cn(
		'inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors',
		'focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-1 focus:ring-offset-background',
		'disabled:cursor-not-allowed disabled:opacity-50',
		variantCls[variant],
		sizeCls[size],
		klass
	)}
>
	{@render children()}
</button>
