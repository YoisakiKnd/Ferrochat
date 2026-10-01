import { expect, test, type Page } from '@playwright/test';
import { login } from './auth';

test.use({
	launchOptions: { args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] },
	permissions: ['microphone']
});
const api = async (page: Page, token: string, method: string, path: string, data?: unknown) => {
	const response = await page.request.fetch(`/api/v1${path}`, {
		method,
		headers: { Authorization: `Bearer ${token}` },
		data
	});
	expect(response.ok(), path).toBeTruthy();
	return response.json();
};
const plainInput = (page: Page) =>
	page.addInitScript(() => {
		window.requestIdleCallback = () => 0;
	});

test('sharing reports failures, keeps a valid link, freezes content and supports anonymous revocation', async ({
	page,
	browser
}) => {
	await page.addInitScript(() => {
		Object.defineProperty(navigator, 'clipboard', {
			value: {
				writeText: async () => {
					throw new DOMException('Clipboard denied', 'NotAllowedError');
				}
			},
			configurable: true
		});
	});
	const token = await login(page, 'ada@example.com', 'Ada');
	const chat = await api(page, token, 'POST', '/chats/new', {
		chat: {
			title: 'Public snapshot',
			models: ['mock:mock-model'],
			messages: [{ id: 'u', role: 'user', content: 'Original shared message' }],
			history: {
				currentId: 'u',
				messages: {
					u: {
						id: 'u',
						role: 'user',
						content: 'Original shared message',
						parentId: null,
						childrenIds: [],
						done: true
					}
				}
			}
		}
	});
	await page.goto(`/c/${chat.id}`);
	await page.locator('#chat-context-menu-button').click();
	await page.getByRole('menuitem', { name: /Share|分享/, exact: true }).click();
	await page.route(`**/api/v1/chats/${chat.id}/share`, (route) =>
		route.fulfill({
			status: 503,
			contentType: 'text/plain',
			body: 'Sharing temporarily unavailable'
		})
	);
	await page.locator('#copy-and-share-chat-button').click();
	await expect(page.getByText('Sharing temporarily unavailable', { exact: true })).toBeVisible();
	await expect(page.locator('#copy-and-share-chat-button')).toBeEnabled();
	await page.unroute(`**/api/v1/chats/${chat.id}/share`);
	await page.locator('#copy-and-share-chat-button').click();
	const urlInput = page.getByRole('textbox', { name: /Shared chat URL|分享聊天链接/ });
	await expect(urlInput).toBeVisible();
	const url = await urlInput.inputValue();
	const owned = await api(page, token, 'GET', `/chats/${chat.id}`);
	expect(url).toContain(`/s/${owned.share_id}`);
	expect(owned.share_id).not.toBe(chat.id);
	await api(page, token, 'POST', `/chats/${chat.id}`, {
		chat: {
			title: 'Private later title',
			messages: [{ role: 'user', content: 'Private later message' }]
		}
	});
	const anonymous = await browser.newContext();
	const visitor = await anonymous.newPage();
	try {
		await visitor.goto(url);
		await expect(visitor.getByText('Original shared message', { exact: true })).toBeVisible();
		await expect(visitor.getByRole('heading', { name: 'Public snapshot' })).toBeVisible();
		await expect(visitor.getByText('Private later message', { exact: true })).toHaveCount(0);
		await expect(visitor).toHaveURL(url);
		await page.evaluate(() => {
			navigator.clipboard.writeText = async (text) => {
				window['copiedShareUrl'] = text;
			};
		});
		await page.locator('#copy-and-share-chat-button').click();
		await expect(page.locator('#copy-and-share-chat-button')).toHaveCount(0);
		expect(await page.evaluate(() => window['copiedShareUrl'])).toBe(url);
		await page.locator('#chat-context-menu-button').click();
		await page.getByRole('menuitem', { name: /Share|分享/, exact: true }).click();
		await page.getByRole('button', { name: /delete this link|删除此链接/, exact: true }).click();
		await expect(
			page.getByRole('button', { name: /Copy Link|复制链接/, exact: true })
		).toBeVisible();
		await visitor.reload();
		await expect(visitor.getByRole('alert')).toHaveText('shared chat not found');
		await expect(visitor.getByRole('button', { name: /Retry|重试/, exact: true })).toBeVisible();
	} finally {
		await anonymous.close();
		await api(page, token, 'DELETE', `/chats/${chat.id}`);
	}
});

test('chat import rejects malformed files, reports server errors and allows retrying the same file', async ({
	page
}) => {
	const token = await login(page, 'ada@example.com', 'Ada');
	await page.locator('#sidebar-toggle-button').click();
	await page.getByRole('button', { name: /Ada/ }).last().click();
	await page.getByRole('button', { name: /Settings|设置/, exact: true }).click();
	await page
		.locator('#settings-tabs-container')
		.getByRole('button', { name: /Chats|聊天/, exact: true })
		.click();
	const input = page.locator('#chat-import-input');
	const file = (body: string) => ({
		name: 'chat-import.json',
		mimeType: 'application/json',
		buffer: Buffer.from(body)
	});
	await input.setInputFiles(file('[null]'));
	await expect(
		page.getByText(/Invalid file format\.|无效的文件格式/, { exact: true })
	).toBeVisible();
	const valid = file(
		JSON.stringify([
			{ chat: { title: 'Imported retry chat', messages: [] }, meta: { note: 'retained' } }
		])
	);
	await page.route('**/api/v1/chats/import', (route) =>
		route.fulfill({
			status: 503,
			contentType: 'text/plain',
			body: 'Import temporarily unavailable'
		})
	);
	await input.setInputFiles(valid);
	await expect(page.getByText('Import temporarily unavailable', { exact: true })).toBeVisible();
	await page.unroute('**/api/v1/chats/import');
	await input.setInputFiles(valid);
	await expect(page.getByText(/Imported 1 chats\.|已导入 1 个聊天/, { exact: true })).toBeVisible();
	const imported = (await api(page, token, 'GET', '/chats/all')).find(
		(chat) => chat.title === 'Imported retry chat'
	);
	expect(imported.meta.note).toBe('retained');
	await api(page, token, 'DELETE', `/chats/${imported.id}`);
});

test('microphone permission failures restore the composer and keep the draft', async ({ page }) => {
	await plainInput(page);
	await page.addInitScript(() => {
		navigator.mediaDevices.getUserMedia = async () => {
			throw new DOMException('Microphone denied', 'NotAllowedError');
		};
	});
	const token = await login(page, 'ada@example.com', 'Ada');
	await api(page, token, 'POST', '/audio/config', { stt_engine: 'openai' });
	await page.reload();
	await page.locator('#chat-input').fill('Keep the voice draft');
	await page.getByRole('button', { name: /Record voice|录音/, exact: true }).click();
	await expect(page.getByText('NotAllowedError: Microphone denied', { exact: true })).toBeVisible();
	await expect(page.locator('#chat-input')).toHaveValue('Keep the voice draft');
	await expect(page.getByRole('button', { name: /Record voice|录音/, exact: true })).toBeVisible();
});

test('cancelling before microphone permission resolves releases the late stream', async ({
	page
}) => {
	await plainInput(page);
	await page.addInitScript(() => {
		window['stoppedTracks'] = 0;
		navigator.mediaDevices.getUserMedia = () =>
			new Promise((resolve) => {
				window['resolveMicrophone'] = () =>
					resolve({
						getTracks: () => [{ stop: () => window['stoppedTracks']++ }]
					} as unknown as MediaStream);
			});
	});
	const token = await login(page, 'ada@example.com', 'Ada');
	await api(page, token, 'POST', '/audio/config', { stt_engine: 'openai' });
	await page.reload();
	await page.getByRole('button', { name: /Record voice|录音/, exact: true }).click();
	await page.getByRole('button', { name: /Cancel recording|取消录音/, exact: true }).click();
	await page.evaluate(() => window['resolveMicrophone']());
	await expect.poll(() => page.evaluate(() => window['stoppedTracks'])).toBe(1);
	await expect(page.locator('#chat-input')).toBeVisible();
});

test('transcription failures use the real recorder format and stop the microphone', async ({
	page
}) => {
	await plainInput(page);
	await page.addInitScript(() => {
		const original = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
		navigator.mediaDevices.getUserMedia = async (options) => {
			const stream = await original(options);
			window['recordedStream'] = stream;
			return stream;
		};
	});
	const token = await login(page, 'ada@example.com', 'Ada');
	await api(page, token, 'POST', '/audio/config', { stt_engine: 'openai' });
	await page.reload();
	let upload = '';
	await page.route('**/api/v1/audio/transcriptions', (route) => {
		upload = route.request().postDataBuffer()?.toString() ?? '';
		return route.fulfill({
			status: 503,
			contentType: 'text/plain',
			body: 'Transcription temporarily unavailable'
		});
	});
	await page.getByRole('button', { name: /Record voice|录音/, exact: true }).click();
	await page.getByRole('button', { name: /Confirm recording|确认录音/, exact: true }).click();
	await expect(
		page.getByText('Transcription temporarily unavailable', { exact: true })
	).toBeVisible();
	expect(upload).toContain('filename="recording.webm"');
	expect(upload).toContain('Content-Type: audio/webm');
	await expect(page.locator('#chat-input')).toBeVisible();
	expect(
		await page.evaluate(() =>
			(window['recordedStream'] as MediaStream)
				.getTracks()
				.every((track) => track.readyState === 'ended')
		)
	).toBe(true);
	await api(page, token, 'POST', '/audio/config', { stt_engine: 'web' });
});

test('browser speech cancellation does not insert text and a new recording can be confirmed', async ({
	page
}) => {
	await plainInput(page);
	await page.addInitScript(() => {
		class Recognition {
			continuous = false;
			onresult = null;
			onerror = null;
			onend = null;
			start() {
				window['activeRecognition'] = this;
			}
			stop() {
				this.onend?.();
			}
		}
		window['SpeechRecognition'] = Recognition;
	});
	const token = await login(page, 'ada@example.com', 'Ada');
	await api(page, token, 'POST', '/audio/config', { stt_engine: 'web' });
	await page.reload();
	await page.locator('#chat-input').fill('Original draft');
	await page.getByRole('button', { name: /Record voice|录音/, exact: true }).click();
	await expect(
		page.getByRole('button', { name: /Confirm recording|确认录音/, exact: true })
	).toBeVisible();
	await page.evaluate(() =>
		window['activeRecognition'].onresult({
			resultIndex: 0,
			results: [[{ transcript: 'Cancelled words' }]]
		})
	);
	await page.getByRole('button', { name: /Cancel recording|取消录音/, exact: true }).click();
	await expect(page.locator('#chat-input')).toHaveValue('Original draft');
	await page.getByRole('button', { name: /Record voice|录音/, exact: true }).click();
	await expect(
		page.getByRole('button', { name: /Confirm recording|确认录音/, exact: true })
	).toBeVisible();
	await page.evaluate(() =>
		window['activeRecognition'].onresult({
			resultIndex: 0,
			results: [[{ transcript: 'Confirmed words' }]]
		})
	);
	await page.getByRole('button', { name: /Confirm recording|确认录音/, exact: true }).click();
	await expect
		.poll(() =>
			page.locator('#chat-input').evaluate((element) => {
				const text = element instanceof HTMLTextAreaElement ? element.value : element.textContent;
				return text?.trimEnd();
			})
		)
		.toBe('Original draft Confirmed words');
});
