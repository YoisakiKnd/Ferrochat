import { expect, test, type Page, type WebSocketRoute } from '@playwright/test';
import { login } from './auth';

const call = async (page: Page, token: string, method: string, path: string, data?: unknown) => {
	const response = await page.request.fetch(`/api/v1${path}`, {
		method,
		headers: { Authorization: `Bearer ${token}` },
		data
	});
	expect(response.ok(), `${method} ${path}`).toBeTruthy();
	return response.json();
};

const usePlainInput = async (page: Page) => {
	await page.addInitScript(() => {
		window.requestIdleCallback = () => 0;
	});
};

const enableMock = async (page: Page, token: string) => {
	await call(page, token, 'POST', '/providers', {
		id: 'mock',
		type: 'mock',
		name: 'Mock',
		enabled: true,
		api_keys: '',
		base_url: ''
	});
	await call(page, token, 'POST', '/providers/mock/models/batch', {
		models: [{ model_id: 'mock-model' }]
	});
};

test('folder drag, collapse persistence, export and deletion keep chats accessible', async ({
	page
}) => {
	const token = await login(page, 'ada@example.com', 'Ada');
	const child = await call(page, token, 'POST', '/folders/', { name: 'UI Child' });
	const chat = await call(page, token, 'POST', '/chats/new', {
		chat: { title: 'UI folder chat', history: { messages: {} } }
	});
	await page.reload();
	await page.locator('#sidebar-toggle-button').click();
	await page.getByRole('button', { name: /Chats|聊天/, exact: true }).hover();
	await page.getByRole('button', { name: /New Folder|新建文件夹/, exact: true }).click();
	const nameInput = page.locator('input[id^="folder-"][id$="-input"]');
	await nameInput.fill('UI Parent');
	await nameInput.press('Enter');
	await expect(nameInput).toHaveCount(0);
	await expect
		.poll(async () =>
			(await call(page, token, 'GET', '/folders/')).some((folder) => folder.name === 'UI Parent')
		)
		.toBe(true);
	const parent = (await call(page, token, 'GET', '/folders/')).find(
		(folder) => folder.name === 'UI Parent'
	);
	const folder = page.locator(`[data-folder-id="${parent.id}"]`);
	await expect(page.locator(`#folder-${parent.id}-button`)).toBeVisible();
	const drag = async (data: unknown) => {
		const transfer = await page.evaluateHandle((value) => {
			const transfer = new DataTransfer();
			transfer.setData('text/plain', JSON.stringify(value));
			return transfer;
		}, data);
		await folder.dispatchEvent('drop', { dataTransfer: transfer });
		await transfer.dispose();
	};
	await drag({ type: 'chat', id: chat.id });
	await expect(folder.locator(`a[href="/c/${chat.id}"]`)).toBeVisible();
	await expect(page.locator(`a[href="/c/${chat.id}"]`)).toHaveCount(1);
	await expect
		.poll(async () => (await call(page, token, 'GET', `/folders/${parent.id}`)).is_expanded)
		.toBe(true);
	await page.locator(`#folder-${parent.id}-button`).click();
	await expect
		.poll(async () => (await call(page, token, 'GET', `/folders/${parent.id}`)).is_expanded)
		.toBe(false);
	await page.reload();
	await expect(folder.locator(`a[href="/c/${chat.id}"]`)).toBeHidden();
	await drag({ type: 'folder', id: child.id });
	await expect(folder.locator(`#folder-${child.id}-button`)).toBeVisible();
	const cycle = await page.request.post(`/api/v1/folders/${parent.id}/update/parent`, {
		headers: { Authorization: `Bearer ${token}` },
		data: { parent_id: child.id }
	});
	expect(cycle.status()).toBe(400);
	await page.locator(`#folder-${parent.id}-button`).hover();
	await folder
		.getByRole('button', { name: /More|更多/, exact: true })
		.first()
		.click();
	const download = page.waitForEvent('download');
	await page.getByRole('menuitem', { name: /Export|导出/, exact: true }).click();
	expect((await download).suggestedFilename()).toContain('UI Parent');
	await page.locator(`#folder-${parent.id}-button`).hover();
	await folder
		.getByRole('button', { name: /More|更多/, exact: true })
		.first()
		.click();
	await page.getByRole('menuitem', { name: /Delete|删除/, exact: true }).click();
	await expect(
		page.getByText(/Its chats and subfolders will be moved|其中的聊天和子文件夹将移回侧栏/)
	).toBeVisible();
	await page.getByRole('button', { name: /Confirm|确认/, exact: true }).click();
	await expect(page.locator(`#folder-${parent.id}-button`)).toHaveCount(0);
	await page.reload();
	await expect(page.locator(`a[href="/c/${chat.id}"]`)).toBeVisible();
	await expect(page.locator(`#folder-${child.id}-button`)).toBeVisible();
	await call(page, token, 'DELETE', `/folders/${child.id}`);
	await call(page, token, 'DELETE', `/chats/${chat.id}`);
});

test('unavailable chat connection preserves the draft instead of starting a spinner', async ({
	page
}) => {
	await usePlainInput(page);
	const token = await login(page, 'ada@example.com', 'Ada');
	await enableMock(page, token);
	await page.routeWebSocket('**/ws/socket.io/**', (socket) => socket.close());
	await page.route('**/ws/socket.io/**', (route) => route.abort());
	await page.reload();
	const input = page.locator('#chat-input');
	await input.fill('Keep this draft');
	await input.press('Enter');
	await expect(page.getByText(/Chat connection is unavailable|聊天连接暂不可用/)).toBeVisible();
	await expect(input).toHaveValue('Keep this draft');
	await expect(page.locator('#stop-response-button')).toHaveCount(0);
});

test('stop works while the completion request is still pending', async ({ page }) => {
	await usePlainInput(page);
	const token = await login(page, 'ada@example.com', 'Ada');
	await enableMock(page, token);
	await page.reload();
	let started!: () => void;
	const requestStarted = new Promise<void>((resolve) => {
		started = resolve;
	});
	let release!: () => void;
	const released = new Promise<void>((resolve) => {
		release = resolve;
	});
	await page.route('**/api/chat/completions', async (route) => {
		started();
		await released;
		await route.fulfill({ json: { status: true, task_id: 'ui-delayed-task' } });
	});
	const input = page.locator('#chat-input');
	await input.fill('Stop before the task ID arrives');
	await input.press('Enter');
	await requestStarted;
	await page.locator('#stop-response-button').click();
	await expect(page.locator('#stop-response-button')).toHaveCount(0);
	const cancelled = page.waitForRequest('**/api/tasks/stop/ui-delayed-task');
	release();
	await cancelled;
	await expect(page.locator('#stop-response-button')).toHaveCount(0);
	await expect(input).toBeVisible();
});

test('losing the connection finishes a pending reply and cancels its late task', async ({
	page
}) => {
	await usePlainInput(page);
	const token = await login(page, 'ada@example.com', 'Ada');
	await enableMock(page, token);
	let connection!: WebSocketRoute;
	let server!: WebSocketRoute;
	let blockReconnect = false;
	let connected!: () => void;
	const ready = new Promise<void>((resolve) => {
		connected = resolve;
	});
	await page.routeWebSocket('**/ws/socket.io/**', (socket) => {
		if (blockReconnect) {
			socket.close();
			return;
		}
		connection = socket;
		server = socket.connectToServer();
		server.onMessage((message) => {
			socket.send(message);
			if (message.toString().startsWith('40')) connected();
		});
	});
	await page.reload();
	await ready;
	let started!: () => void;
	const requestStarted = new Promise<void>((resolve) => {
		started = resolve;
	});
	let release!: () => void;
	const released = new Promise<void>((resolve) => {
		release = resolve;
	});
	await page.route('**/api/chat/completions', async (route) => {
		started();
		await released;
		await route.fulfill({ json: { status: true, task_id: 'ui-disconnected-task' } });
	});
	await page.locator('#chat-input').fill('Interrupt this reply');
	await page.locator('#chat-input').press('Enter');
	await requestStarted;
	await expect(page.locator('#stop-response-button')).toBeVisible();
	blockReconnect = true;
	await connection.close();
	await server.close();
	await expect(
		page.getByText(/Connection lost. Please retry your message.|连接已断开，请重新生成回复/).first()
	).toBeVisible();
	await expect(page.locator('#stop-response-button')).toHaveCount(0);
	const cancelled = page.waitForRequest('**/api/tasks/stop/ui-disconnected-task');
	release();
	await cancelled;
	await expect(page.locator('#stop-response-button')).toHaveCount(0);
});

test('plain-text upload errors are visible and failed attachments are removed', async ({
	page
}) => {
	await login(page, 'ada@example.com', 'Ada');
	await page.route('**/api/v1/files/', (route) =>
		route.fulfill({
			status: 413,
			contentType: 'text/plain',
			body: 'Upload rejected by proxy'
		})
	);
	await page
		.locator('input[type="file"][multiple]')
		.first()
		.setInputFiles({
			name: 'ui-note.txt',
			mimeType: 'text/plain',
			buffer: Buffer.from('note')
		});
	await expect(page.getByText('Upload rejected by proxy', { exact: true })).toBeVisible();
	await expect(page.getByText('ui-note.txt', { exact: true })).toHaveCount(0);
});

test('password validation failures preserve fields for retry', async ({ page }) => {
	await login(page, 'ada@example.com', 'Ada');
	await page.locator('#sidebar-toggle-button').click();
	await page.getByRole('button', { name: /Ada/ }).last().click();
	await page.getByRole('button', { name: /Settings|设置/, exact: true }).click();
	await page
		.locator('#settings-tabs-container')
		.getByRole('button', { name: /Account|账户|账号/, exact: true })
		.click();
	const form = page
		.locator('form')
		.filter({ has: page.getByText(/Change Password|修改密码|更改密码/, { exact: true }) });
	await form.getByRole('button', { name: /Show|显示/, exact: true }).click();
	const current = form.locator('input[autocomplete="current-password"]');
	const next = form.locator('input[autocomplete="new-password"]');
	const confirm = form.locator('input[autocomplete="off"]');
	await current.fill('wrong-current');
	await next.fill('secret2');
	await confirm.fill('secret2');
	await form.getByRole('button', { name: /Update password|更新密码/, exact: true }).click();
	await expect(page.getByText('current password is wrong', { exact: true })).toBeVisible();
	await expect(current).toHaveValue('wrong-current');
	await expect(next).toHaveValue('secret2');
	await expect(confirm).toHaveValue('secret2');
});
