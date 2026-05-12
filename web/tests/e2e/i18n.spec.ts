/**
 * /login + chrome i18n toggle (M10 / ADR-0011).
 */
import { test, expect } from '@playwright/test';

test.beforeEach(async ({ page }) => {
	await page.route('**/api/version', (route) =>
		route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({
				studio_server: '0.4.0-dev',
				studio_store: '0.4.0-dev',
				studio_router: '0.4.0-dev',
				rustc: 'test'
			})
		})
	);
	await page.route('**/api/project/current', (route) =>
		route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({
				project_root: '/tmp/cobrust-studio',
				started_at: '2026-05-13T00:00:00Z',
				version: 'test'
			})
		})
	);
});

test('language toggle switches login chrome to Chinese and persists across reload', async ({
	page
}) => {
	await page.goto('/login');
	await expect(
		page.getByText('Configure your LLM endpoint to start dispatching agents.')
	).toBeVisible();

	await page.getByRole('button', { name: '中' }).click();
	await expect(page.getByText('配置 LLM 端点后即可开始调度 agent。')).toBeVisible();

	await page.reload();
	await expect(page.getByText('配置 LLM 端点后即可开始调度 agent。')).toBeVisible();
});

test('language choice carries into app page chrome', async ({ page }) => {
	await page.goto('/login');
	await page.getByRole('button', { name: '中' }).click();
	await page.goto('/agent');
	await expect(page.getByRole('link', { name: '账本' })).toBeVisible();
});
