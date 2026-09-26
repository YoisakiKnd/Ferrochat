import { expect, test } from '@playwright/test';
import { login } from './auth';

// These endpoints were 404s or stubs while the frontend already called them.
// Keep the URL paths and body shapes identical to `$lib/apis/chats` so a rename
// on either side fails here instead of shipping a dead button.
// Titles are tagged @chats: CI runs them only on release tags (see .github/workflows/ci.yml).

test('@chats archive toggles and the archived lists agree', async ({ page }) => {
	const token = await login(page, 'ada@example.com', 'Ada');
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
	const token = await login(page, 'ada@example.com', 'Ada');
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
