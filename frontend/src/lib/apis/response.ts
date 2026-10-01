// Keep network failures and non-JSON proxy errors visible to callers.
export const fetchResponse = async (url: string, init?: RequestInit): Promise<Response> => {
	let response: Response;
	try {
		response = await fetch(url, init);
	} catch (error) {
		throw error instanceof Error ? error.message : String(error);
	}
	if (!response.ok) {
		const text = await response.text();
		let detail = text || `Request failed (${response.status})`;
		try {
			detail = JSON.parse(text).detail || detail;
		} catch {
			// Proxies may return a plain-text error.
		}
		throw detail;
	}
	return response;
};
