import { expect, type Page } from '@playwright/test';

// Whichever auth view hydrates first wins; a plain count() check races
// client-side rendering and mis-detects the first-run signup page.
export const firstRunView = (page: Page) =>
	Promise.race([
		page.getByRole('button', { name: /get started|开始使用/i }).waitFor({ timeout: 10000 }).then(() => true),
		page.getByRole('button', { name: /sign in|登录/i }).waitFor({ timeout: 10000 }).then(() => false)
	]);

export async function login(
	page: Page,
	signupEmail: string,
	signupName: string,
	loginEmail = signupEmail
): Promise<string> {
	await page.goto('/auth');
	if (await firstRunView(page)) {
		await page.getByRole('button', { name: /get started|开始使用/i }).click();
		await page.getByPlaceholder(/email|邮箱/i).fill(signupEmail);
		await page.getByPlaceholder(/password|密码/i).fill('secret1');
		const name = page.getByPlaceholder(/name|名称/i);
		if (await name.count()) await name.fill(signupName);
		await page.getByRole('button', { name: /create admin account|创建管理员账[号户]/i }).click();
	} else {
		await page.getByPlaceholder(/email|邮箱/i).fill(loginEmail);
		await page.getByPlaceholder(/password|密码/i).fill('secret1');
		await page.getByRole('button', { name: /sign in|登录/i }).click();
	}
	await expect(page.locator('#chat-input')).toBeVisible();
	return page.evaluate(() => localStorage.token);
}
