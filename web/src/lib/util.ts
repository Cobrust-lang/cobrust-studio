/**
 * Small leaf utilities shared across components — kept here so each
 * page doesn't roll its own tailwind class merger or timestamp
 * formatter.
 */

/**
 * Merge tailwind-style class strings. Falsy entries are skipped. We
 * deliberately do not pull `tailwind-merge` for this — the page
 * surface is small enough that whitespace-joined strings are fine.
 */
export function cn(...parts: Array<string | false | null | undefined>): string {
	return parts.filter(Boolean).join(' ');
}

/** Render an RFC-3339 timestamp as a compact `YYYY-MM-DD HH:MM:SS` UTC. */
export function fmtTs(iso: string): string {
	const d = new Date(iso);
	if (Number.isNaN(d.getTime())) return iso;
	const pad = (n: number) => String(n).padStart(2, '0');
	return (
		`${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ` +
		`${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`
	);
}

/**
 * Map an ADR status to a tailwind colour class. Unknown statuses
 * land on the muted/info colour so the UI doesn't blank-cell.
 */
export function adrStatusClass(status: string): string {
	switch (status.toLowerCase()) {
		case 'accepted':
			return 'bg-[hsl(var(--ok)/0.18)] text-[hsl(var(--ok))]';
		case 'proposed':
			return 'bg-[hsl(var(--info)/0.18)] text-[hsl(var(--info))]';
		case 'superseded':
			return 'bg-[hsl(var(--warn)/0.18)] text-[hsl(var(--warn))]';
		case 'deprecated':
			return 'bg-[hsl(var(--err)/0.18)] text-[hsl(var(--err))]';
		default:
			return 'bg-secondary text-secondary-foreground';
	}
}

/**
 * Map a Finding severity (P0/P1/P2/P3) to a tailwind colour class.
 */
export function severityClass(sev: string): string {
	switch (sev.toUpperCase()) {
		case 'P0':
			return 'bg-[hsl(var(--err)/0.22)] text-[hsl(var(--err))]';
		case 'P1':
			return 'bg-[hsl(var(--warn)/0.22)] text-[hsl(var(--warn))]';
		case 'P2':
			return 'bg-[hsl(45_92%_55%/0.22)] text-[hsl(45_92%_70%)]';
		case 'P3':
			return 'bg-[hsl(var(--info)/0.18)] text-[hsl(var(--info))]';
		default:
			return 'bg-secondary text-secondary-foreground';
	}
}

/**
 * Map a Finding status to a tailwind colour class. `open` is warn-ish
 * (needs work), `closed_by_*` is ok-ish (work done).
 */
export function findingStatusClass(status: string): string {
	const lower = status.toLowerCase();
	if (lower === 'open') return 'bg-[hsl(var(--warn)/0.18)] text-[hsl(var(--warn))]';
	if (lower.startsWith('closed_by')) return 'bg-[hsl(var(--ok)/0.18)] text-[hsl(var(--ok))]';
	return 'bg-secondary text-secondary-foreground';
}
