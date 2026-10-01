<script>
	import { getContext } from 'svelte';
	import { page } from '$app/stores';
	import { getChatByShareId } from '$lib/apis/chats';

	let chat = null;
	let error = '';
	const i18n = getContext('i18n');
	let requestId = 0;

	const loadChat = async (id) => {
		const request = ++requestId;
		chat = null;
		error = '';
		try {
			const result = await getChatByShareId('', id);
			if (request === requestId) chat = result;
		} catch (err) {
			if (request === requestId) error = String(err);
		}
	};
	$: loadChat($page.params.id);

	$: messages = chat?.chat?.messages ?? chat?.chat?.history?.messages ?? [];
	$: list = Array.isArray(messages) ? messages : Object.values(messages ?? {});
</script>

<div class="max-w-3xl mx-auto p-6">
	{#if error}
		<p role="alert">{error}</p>
		<button on:click={() => loadChat($page.params.id)}>{$i18n.t('Retry')}</button>
	{:else if !chat}
		<p>{$i18n.t('Loading...')}</p>
	{:else}
		<h1 class="text-xl font-medium mb-4">{chat.title ?? 'Shared chat'}</h1>
		{#each list as message}
			<div class="mb-3">
				<div class="text-xs text-gray-500">{message.role}</div>
				<div class="whitespace-pre-wrap">
					{typeof message.content === 'string' ? message.content : ''}
				</div>
			</div>
		{/each}
	{/if}
</div>
