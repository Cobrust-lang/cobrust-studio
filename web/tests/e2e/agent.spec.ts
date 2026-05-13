import { test, expect, type Page } from '@playwright/test';

const APP_VERSION = {
	studio_server: '0.4.0-dev',
	studio_store: '0.4.0-dev',
	studio_router: '0.4.0-dev',
	rustc: 'test'
} as const;

const APP_PROJECT = {
	project_root: '/tmp/cobrust-studio',
	started_at: '2026-05-13T00:00:00Z',
	version: 'test'
} as const;

const SESSION_STATUS = {
	authenticated: true,
	selected_model: 'claude-test-1',
	provider_kind: 'anthropic'
} as const;

const SESSION_MODELS = {
	provider_kind: 'anthropic',
	selected_model: 'claude-test-1',
	models: ['claude-test-1', 'claude-test-2']
} as const;

const READ_ONLY_TOOLS = ['fs.read', 'fs.list', 'git.status', 'git.diff', 'project_tree'];

const AGENT_TURN_SSE = [
	'event: iteration',
	`data: ${JSON.stringify({
		n: 0,
		model: 'claude-test-1',
		text: JSON.stringify({
			type: 'tool_use',
			calls: [{ tool: 'fs.read', input: { path: 'README.md' } }]
		}),
		usage: { prompt_tokens: 10, completion_tokens: 5 },
		cache_hit: false,
		stop_reason: 'tool_use'
	})}`,
	'',
	'event: tool_call',
	`data: ${JSON.stringify({ iteration: 0, tool: 'fs.read', input: { path: 'README.md' } })}`,
	'',
	'event: tool_result',
	`data: ${JSON.stringify({
		iteration: 0,
		tool: 'fs.read',
		output: { path: 'README.md', content: '# Cobrust Studio\n' },
		error: null,
		ms: 12
	})}`,
	'',
	'event: iteration',
	`data: ${JSON.stringify({
		n: 1,
		model: 'claude-test-1',
		text: JSON.stringify({ type: 'final', text: 'ADR status: stable.' }),
		usage: { prompt_tokens: 8, completion_tokens: 4 },
		cache_hit: true,
		stop_reason: 'end_turn'
	})}`,
	'',
	'event: done',
	`data: ${JSON.stringify({
		final_text: 'ADR status: stable.',
		iterations: 2,
		total_tokens: { prompt_tokens: 18, completion_tokens: 9 },
		task_tag: 'agent-turn'
	})}`,
	''
].join('\n');

async function stubAppShell(page: Page) {
	await page.route('**/api/version', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(APP_VERSION)
		});
	});
	await page.route('**/api/project/current', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(APP_PROJECT)
		});
	});
}

test('missing session shows the configure endpoint CTA', async ({ page }) => {
	await stubAppShell(page);
	await page.route('**/api/session/status', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({
				authenticated: false,
				selected_model: null,
				provider_kind: null
			})
		});
	});
	await page.goto('/agent');

	await expect(page.getByText(/LLM router not configured/i)).toBeVisible({ timeout: 5_000 });
	await expect(page.getByRole('link', { name: /Configure endpoint/i })).toHaveAttribute(
		'href',
		'/login'
	);
	await expect(page.getByText(/Run an agent turn to populate the timeline\./i)).toBeVisible();
});

test('authenticated agent turn renders iterations, tool events, and final state', async ({ page }) => {
	let agentTurnRequest: Record<string, unknown> | null = null;
	let dispatchCalled = false;

	await stubAppShell(page);
	await page.route('**/api/session/status', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(SESSION_STATUS)
		});
	});
	await page.route('**/api/models/session', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(SESSION_MODELS)
		});
	});
	await page.route('**/api/dispatch', async (route) => {
		dispatchCalled = true;
		await route.abort();
	});
	await page.route('**/api/agent-turn', async (route) => {
		agentTurnRequest = route.request().postDataJSON() as Record<string, unknown>;
		await route.fulfill({
			status: 200,
			headers: {
				'content-type': 'text/event-stream; charset=utf-8',
				'cache-control': 'no-cache'
			},
			body: AGENT_TURN_SSE
		});
	});

	await page.goto('/agent');
	await expect(page.locator('input[list="agent-model-options"]')).toHaveValue('claude-test-1');

	await page.locator('textarea').nth(1).fill('Summarise the ADR status.');
	await page.getByRole('button', { name: /Run agent turn/i }).click();

	expect(agentTurnRequest).toMatchObject({
		model: 'claude-test-1',
		messages: [{ role: 'user', content: 'Summarise the ADR status.' }],
		max_iterations: 16,
		tools_allowed: READ_ONLY_TOOLS,
		task_tag: 'agent-turn'
	});
	expect(dispatchCalled).toBe(false);

	await expect(page.getByText('Iteration 1')).toBeVisible();
	await expect(page.getByText('Iteration 2')).toBeVisible();
	await expect(page.getByText(/· tool_use/)).toBeVisible();
	await expect(page.getByText(/· end_turn/)).toBeVisible();
	await expect(page.getByText('cache hit')).toBeVisible();
	await expect(
		page.getByText('{"type":"tool_use","calls":[{"tool":"fs.read","input":{"path":"README.md"}}]}')
	).toBeVisible();

	const toolCallSection = page.locator('section').filter({
		has: page.getByText('tool call:')
	});
	const toolResultSection = page.locator('section').filter({
		has: page.getByText('tool result:')
	});

	await expect(toolCallSection).toBeVisible();
	await expect(toolResultSection).toBeVisible();
	await expect(toolCallSection.getByText(/"path": "README.md"/)).toBeVisible();
	await expect(toolResultSection.getByText(/"path": "README.md"/)).toBeVisible();
	await expect(toolResultSection.getByText(/"content": "# Cobrust Studio\\n"/)).toBeVisible();
	await expect(toolResultSection.getByText('12ms')).toBeVisible();
	const finalStateSection = page.locator('section').filter({
		has: page.getByText(/Final text|最终答复/)
	});
	await expect(finalStateSection).toBeVisible();
	await expect(finalStateSection.getByText('ADR status: stable.')).toBeVisible();
	await expect(page.getByText('iterations: 2')).toBeVisible();
	await expect(page.getByText('tool calls: 1')).toBeVisible();
	await expect(page.getByText('tokens: 18+9')).toBeVisible();
	await expect(page.getByText('tag: agent-turn')).toBeVisible();
});
