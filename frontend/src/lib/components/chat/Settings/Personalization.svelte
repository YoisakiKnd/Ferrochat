<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { settings } from '$lib/stores';
	import {
		addNewMemory,
		deleteMemoryById,
		getMemories
	} from '$lib/apis/memories';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	export let saveSettings: Function;

	let enabled = false;
	let content = '';
	let items: { id: string; content: string }[] = [];

	async function load() {
		try {
			items = (await getMemories(localStorage.token)) ?? [];
		} catch (error) {
			toast.error(String(error));
		}
	}

	async function add() {
		const text = content.trim();
		if (!text) return;
		try {
			await addNewMemory(localStorage.token, text);
			content = '';
			await load();
		} catch (error) {
			toast.error(String(error));
		}
	}

	async function remove(id: string) {
		try {
			await deleteMemoryById(localStorage.token, id);
			await load();
		} catch (error) {
			toast.error(String(error));
		}
	}

	onMount(() => {
		enabled = $settings?.memory ?? false;
		load();
	});
</script>

<div class="mt-3">
	<div class="flex items-center justify-between text-sm">
		<div class="font-medium">{$i18n.t('Memory')}</div>
		<button
			class="p-1 px-3 text-xs flex rounded-sm transition"
			class:bg-emerald-600={enabled}
			class:text-white={enabled}
			class:bg-gray-50={!enabled}
			class:dark:bg-gray-800={!enabled}
			type="button"
			on:click={() => {
				enabled = !enabled;
				saveSettings({ memory: enabled });
			}}
		>
			{enabled ? $i18n.t('On') : $i18n.t('Off')}
		</button>
	</div>
	<div class="mt-1 text-xs text-gray-500">
		{$i18n.t('Relevant memories are added to the chat when this is on.')}
	</div>
	<div class="mt-3 space-y-1">
		<label class="flex items-center justify-between text-sm">
			<span>{$i18n.t('Suggest memories')}</span>
			<input
				type="checkbox"
				checked={$settings?.memorySuggest ?? true}
				on:change={(event) => saveSettings({ memorySuggest: event.currentTarget.checked })}
			/>
		</label>
		<div class="text-xs text-gray-500">
			{$i18n.t('The assistant can propose a memory after a chat. You confirm it before it is saved.')}
		</div>
	</div>
	<div class="mt-2 flex gap-2">
		<input
			class="flex-1 rounded-lg bg-gray-50 dark:bg-gray-900 px-3 py-2 text-sm"
			bind:value={content}
			placeholder={$i18n.t('Add a memory')}
			on:keydown={(event) => {
				if (event.key === 'Enter') add();
			}}
		/>
		<button class="px-3 py-1.5 rounded-lg bg-gray-900 text-white dark:bg-white dark:text-black text-sm" type="button" on:click={add}>
			{$i18n.t('Add')}
		</button>
	</div>
	<ul class="mt-2 space-y-1 text-sm">
		{#each items as item}
			<li class="flex items-start justify-between gap-2">
				<span class="text-gray-700 dark:text-gray-200">{item.content}</span>
				<button class="text-xs text-gray-500" type="button" on:click={() => remove(item.id)}>
					{$i18n.t('Delete')}
				</button>
			</li>
		{/each}
	</ul>
</div>
