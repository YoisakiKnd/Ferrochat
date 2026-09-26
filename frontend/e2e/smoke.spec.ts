import { expect, test, type Page } from '@playwright/test';
import { login } from './auth';

const api = async (page: Page, path: string, body?: unknown) => {
	const token = await page.evaluate(() => localStorage.token);
	const res = await page.request.fetch(`/api/v1${path}`, {
		method: body ? 'POST' : 'GET',
		headers: { Authorization: `Bearer ${token}` },
		data: body
	});
	expect(res.ok(), path).toBeTruthy();
	return res.json();
};

test('signup, provider model picking, models admin, and chat', async ({ page }) => {
	await login(page, 'ada@example.com', 'Ada');
	await expect(page.locator('#chat-input')).toBeVisible();

	await api(page, '/providers', {
		id: 'mock',
		type: 'mock',
		name: 'Mock',
		base_url: '',
		api_keys: '',
		enabled: true,
		sort_order: 0
	});

	await page.goto('/admin/settings?tab=providers');
	await page.getByRole('button', { name: 'Mock', exact: true }).click();
	await page.getByRole('button', { name: /fetch models|获取模型/i }).click();
	const dialog = page.getByTestId('fetch-models');
	await expect(dialog.getByText('mock-gpt-4o', { exact: true })).toBeVisible();
	await dialog.getByRole('button', { name: /select all|全选/i }).last().click();
	if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/fetch-models.png` });
	await dialog.getByRole('button', { name: /add selected|添加所选/i }).click();
	await expect(dialog.getByText(/added|已添加/i).first()).toBeVisible();
	await page.keyboard.press('Escape');

	await page.goto('/admin/settings?tab=models');
	const list = page.getByTestId('models-admin');
	await expect(list.getByText('mock-claude-3-5-sonnet', { exact: true })).toBeVisible();
	const managed = await api(page, '/models/managed');
	expect(managed.models.length).toBe(5);
	if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/models.png` });

	await api(page, '/providers/mock/models/mock-model', { params: { temperature: 0.3 } });
	await api(page, '/models/managed/reorder', {
		ids: ['mock:mock-model', ...managed.models.map((m) => m.id).filter((id) => id !== 'mock:mock-model')]
	});

	await page.goto('/');
	await page.locator('#chat-input').click();
	await page.keyboard.type('hi');
	await page.keyboard.press('Enter');
	await expect(page.getByText('Hello t=0.3')).toBeVisible({ timeout: 20000 });
	await expect(page.locator('.animate-spin')).toHaveCount(0);
	await expect
		.poll(async () => (await api(page, '/chats/')).map((c) => c.title), { timeout: 20000 })
		.toContain('Hello there');

	await page.getByText('Hello t=0.3').hover();
	await page.getByRole('button', { name: /^regenerate$|^重新生成$/i }).last().click();
	await expect(page.getByText('2/2')).toBeVisible({ timeout: 20000 });

	await page.getByText('hi', { exact: true }).hover();
	await page.getByRole('button', { name: /^edit$|^编辑$/i }).first().click();
	const editor = page.locator('textarea').first();
	await editor.fill('hello again');
	await page.getByRole('button', { name: /^send$|^发送$/i }).click();
	await expect(page.getByText('hello again')).toBeVisible();
	await expect(page.getByText('2/2').first()).toBeVisible();
	await expect(page.getByText('Hello t=0.3').last()).toBeVisible({ timeout: 20000 });
});

test('plain textarea can send before rich text loads', async ({ page }) => {
	await page.addInitScript(() => {
		window.requestIdleCallback = () => 0;
	});
	await login(page, 'ada@example.com', 'Ada');
	const input = page.locator('#chat-input');
	await expect(input).toBeVisible();
	await expect.poll(async () => input.evaluate((el) => el.tagName)).toBe('TEXTAREA');
	await input.fill('hi');
	await input.press('Enter');
	// Match the assistant message body, not the auto-generated sidebar title
	// ("Hello there") or the send-suggestion chip ("Hello ") that share the word.
	await expect(page.getByText(/Hello t=/)).toBeVisible({ timeout: 20000 });
});

test('tools page, shortcuts, and a phone-width layout', async ({ page }) => {
	await login(page, 'bea@example.com', 'Bea', 'ada@example.com');
	await expect(page.locator('#chat-input')).toBeVisible();

	await page.goto('/tools');
	await expect(page.getByRole('button', { name: /翻译|Translate/ }).first()).toBeVisible();
	await page.getByRole('button', { name: /润色|Polish/ }).first().click();
	await expect(page.getByRole('button', { name: /润色|Polish/ }).first()).toBeVisible();
	await page.getByRole('button', { name: /总结|Summarize/ }).first().click();
	await expect(page.getByText(/联网搜索|Web Search/)).toBeVisible();

	await page.goto('/');
	await expect(page.locator('#chat-input')).toBeVisible();
	await page.locator('body').click({ position: { x: 8, y: 8 } });
	await page.keyboard.press('Control+Slash');
	await expect(page.getByText(/键盘快捷键|Keyboard shortcuts/)).toBeVisible();
	await page.keyboard.press('Escape');

	await page.setViewportSize({ width: 390, height: 844 });
	await page.locator('#sidebar-toggle-button').click();
	await expect(page.getByRole('link', { name: /工具|Tools/ })).toBeVisible();
	const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 2);
	expect(overflow).toBe(false);
});
