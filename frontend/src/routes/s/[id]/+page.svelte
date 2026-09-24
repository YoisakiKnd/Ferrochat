<script>
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { getChatByShareId } from '$lib/apis/chats';

	let chat = null;
	let error = '';

	onMount(async () => {
		chat = await getChatByShareId(localStorage.token, $page.params.id);
		if (!chat) {
			error = 'Shared chat was not found.';
		}
	});

	$: messages = chat?.chat?.messages ?? chat?.chat?.history?.messages ?? [];
	$: list = Array.isArray(messages) ? messages : Object.values(messages ?? {});
</script>

<div class="max-w-3xl mx-auto p-6">
	{#if error}
		<p>{error}</p>
	{:else if !chat}
		<p>Loading...</p>
	{:else}
		<h1 class="text-xl font-medium mb-4">{chat.title ?? 'Shared chat'}</h1>
		{#each list as message}
			<div class="mb-3">
				<div class="text-xs text-gray-500">{message.role}</div>
				<div class="whitespace-pre-wrap">{typeof message.content === 'string' ? message.content : ''}</div>
			</div>
		{/each}
	{/if}
</div>
