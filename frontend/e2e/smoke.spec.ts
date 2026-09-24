import { expect, test, type Page } from '@playwright/test';

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
	await page.goto('/auth');
	await page.getByRole('button', { name: /get started|开始使用/i }).click();
	await page.getByPlaceholder(/email|邮箱/i).fill('ada@example.com');
	await page.getByPlaceholder(/password|密码/i).fill('secret1');
	const name = page.getByPlaceholder(/name|名称/i);
	if (await name.count()) await name.fill('Ada');
	await page.getByRole('button', { name: /create admin account|创建管理员账[号户]/i }).click();
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
