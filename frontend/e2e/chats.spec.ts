import { expect, test, type Page } from '@playwright/test';

// These endpoints were 404s or stubs while the frontend already called them.
// Keep the URL paths and body shapes identical to `$lib/apis/chats` so a rename
// on either side fails here instead of shipping a dead button.
// Titles are tagged @chats: CI runs them only on release tags (see .github/workflows/ci.yml).

async function login(page: Page): Promise<string> {
	await page.goto('/auth');
	const start = page.getByRole('button', { name: /get started|开始使用/i });
	if (await start.count()) {
		await start.click();
		await page.getByPlaceholder(/email|邮箱/i).fill('ada@example.com');
		await page.getByPlaceholder(/password|密码/i).fill('secret1');
		const name = page.getByPlaceholder(/name|名称/i);
		if (await name.count()) await name.fill('Ada');
		await page.getByRole('button', { name: /create admin account|创建管理员账[号户]/i }).click();
	} else {
		await page.getByPlaceholder(/email|邮箱/i).fill('ada@example.com');
		await page.getByPlaceholder(/password|密码/i).fill('secret1');
		await page.getByRole('button', { name: /sign in|登录/i }).click();
	}
	await expect(page.locator('#chat-input')).toBeVisible();
	return page.evaluate(() => localStorage.token);
}

test('@chats archive toggles and the archived lists agree', async ({ page }) => {
	const token = await login(page);
	const call = async (method: string, path: string, body?: unknown) => {
		const res = await page.request.fetch(`/api/v1${path}`, {
			method,
			headers: { Authorization: `Bearer ${token}`, 'content-type': 'application/json' },
			data: body === undefined ? undefined : JSON.stringify(body)
		});
		expect(res.ok(), `${method} ${path}`).toBeTruthy();
		return res.json();
	};

	const chat = await call('POST', '/chats/new', { chat: { title: 'e2e archive', history: { messages: {} } } });
	const id: string = chat.id;

	const ids = (list: Array<{ id: string }>) => list.map((c) => c.id);
	expect(ids(await call('GET', '/chats/'))).toContain(id);

	await call('POST', `/chats/${id}/archive`);
	expect(ids(await call('GET', '/chats/'))).not.toContain(id);
	expect(ids(await call('GET', '/chats/archived'))).toContain(id);
	expect(ids(await call('GET', '/chats/all'))).not.toContain(id);
	const allArchived = await call('GET', '/chats/all/archived');
	expect(ids(allArchived)).toContain(id);
	expect(allArchived.find((c) => c.id === id).chat.title).toBe('e2e archive');

	await call('POST', `/chats/${id}/archive`);
	expect(ids(await call('GET', '/chats/'))).toContain(id);
	expect(ids(await call('GET', '/chats/archived'))).not.toContain(id);

	expect((await call('POST', '/chats/archive/all')).status).toBe(true);
	expect(ids(await call('GET', '/chats/archived'))).toContain(id);
	await call('POST', `/chats/${id}/archive`);
	expect(ids(await call('GET', '/chats/archived'))).not.toContain(id);
});

test('@chats tag add, filter, and delete roundtrip', async ({ page }) => {
	const token = await login(page);
	const call = async (method: string, path: string, body?: unknown) => {
		const res = await page.request.fetch(`/api/v1${path}`, {
			method,
			headers: { Authorization: `Bearer ${token}`, 'content-type': 'application/json' },
			data: body === undefined ? undefined : JSON.stringify(body)
		});
		expect(res.ok(), `${method} ${path}`).toBeTruthy();
		return res.json();
	};

	const chat = await call('POST', '/chats/new', { chat: { title: 'e2e tags', history: { messages: {} } } });
	const id: string = chat.id;

	const afterAdd = await call('POST', `/chats/${id}/tags`, { name: 'e2e-tag' });
	expect(afterAdd.map((t) => t.name)).toContain('e2e-tag');
	expect((await call('POST', '/chats/tags', { name: 'e2e-tag' })).map((c) => c.id)).toContain(id);
	expect((await call('GET', '/chats/all/tags')).map((t) => t.name)).toContain('e2e-tag');

	const afterDelete = await call('DELETE', `/chats/${id}/tags`, { name: 'e2e-tag' });
	expect(afterDelete).toEqual([]);
	expect(await call('POST', '/chats/tags', { name: 'e2e-tag' })).toEqual([]);
	expect((await call('GET', '/chats/all/tags')).map((t) => t.name)).not.toContain('e2e-tag');
});
