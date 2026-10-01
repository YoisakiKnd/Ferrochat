export type ImportedChat = {
	chat: Record<string, unknown>;
	meta: Record<string, unknown>;
	pinned: boolean;
};

const isObject = (value: unknown): value is Record<string, unknown> =>
	value !== null && typeof value === 'object' && !Array.isArray(value);

// Validate the entire file before sending its first chat to the server.
export const normalizeChatImports = (value: unknown): ImportedChat[] => {
	if (!Array.isArray(value) || value.length === 0) throw new Error('Invalid file format.');
	return value.map((entry) => {
		if (!isObject(entry)) throw new Error('Invalid file format.');
		const chat = 'chat' in entry ? entry.chat : entry;
		if (!isObject(chat) || !('history' in chat || 'messages' in chat)) {
			throw new Error('Invalid file format.');
		}
		if ('history' in chat && (!isObject(chat.history) || !isObject(chat.history.messages))) {
			throw new Error('Invalid file format.');
		}
		if (
			isObject(chat.history) &&
			isObject(chat.history.messages) &&
			!Object.values(chat.history.messages).every(isObject)
		)
			throw new Error('Invalid file format.');
		if ('messages' in chat && (!Array.isArray(chat.messages) || !chat.messages.every(isObject)))
			throw new Error('Invalid file format.');
		return { chat, meta: isObject(entry.meta) ? entry.meta : {}, pinned: entry.pinned === true };
	});
};
