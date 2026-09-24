<script lang="ts">
	import { onMount, getContext } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { WEBUI_API_BASE_URL } from '$lib/constants';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	type Server = {
		id?: string;
		type: 'mcp_stdio' | 'mcp_http' | 'openapi';
		name: string;
		enabled: boolean;
		config: { command?: string; args?: string; url?: string };
	};

	let servers: Server[] = [];
	let draft: Server = { type: 'mcp_http', name: '', enabled: true, config: { url: '' } };

	const headers = () => ({
		Authorization: `Bearer ${localStorage.token}`,
		'Content-Type': 'application/json'
	});

	const load = async () => {
		const res = await fetch(`${WEBUI_API_BASE_URL}/tool-servers`, { headers: headers() });
		servers = await res.json();
	};

	const save = async () => {
		const config = { ...draft.config };
		if (typeof config.args === 'string') {
			config.args = config.args
				.split(' ')
				.map((part) => part.trim())
				.filter(Boolean) as unknown as string;
		}
		const res = await fetch(`${WEBUI_API_BASE_URL}/tool-servers`, {
			method: 'POST',
			headers: headers(),
			body: JSON.stringify({ ...draft, config })
		});
		if (!res.ok) {
			toast.error($i18n.t('Failed to save'));
			return;
		}
		draft = { type: 'mcp_http', name: '', enabled: true, config: { url: '' } };
		await load();
	};

	const remove = async (id: string) => {
		await fetch(`${WEBUI_API_BASE_URL}/tool-servers/${id}`, { method: 'DELETE', headers: headers() });
		await load();
	};

	onMount(load);
</script>

<div class="max-w-3xl mx-auto py-4 space-y-4">
	<div>
		<div class="text-lg font-medium">{$i18n.t('Tools')}</div>
		<div class="text-sm text-gray-500">MCP servers. stdio servers need a command such as npx or uvx. HTTP servers need a URL.</div>
	</div>

	<div class="space-y-2 rounded-xl border border-gray-100 dark:border-gray-800 p-3">
		<input class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" placeholder="Name" bind:value={draft.name} />
		<select class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" bind:value={draft.type}>
			<option value="mcp_http">MCP HTTP</option>
			<option value="mcp_stdio">MCP stdio</option>
			<option value="openapi">OpenAPI</option>
		</select>
		{#if draft.type === 'mcp_stdio'}
			<input class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" placeholder="command" bind:value={draft.config.command} />
			<input class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" placeholder="args" bind:value={draft.config.args} />
		{:else}
			<input class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" placeholder="https://..." bind:value={draft.config.url} />
		{/if}
		<button class="px-3 py-1.5 rounded-lg bg-black text-white text-sm dark:bg-white dark:text-black" on:click={save}
			>{$i18n.t('Save')}</button
		>
	</div>

	{#each servers as server}
		<div class="flex items-center justify-between rounded-xl px-3 py-2 bg-gray-50 dark:bg-gray-900">
			<div>
				<div class="text-sm font-medium">{server.name}</div>
				<div class="text-xs text-gray-500">{server.type} {server.config?.url || server.config?.command || ''}</div>
			</div>
			<button class="text-xs text-red-500" on:click={() => server.id && remove(server.id)}>{$i18n.t('Delete')}</button>
		</div>
	{/each}
</div>
