<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { WEBUI_API_BASE_URL } from '$lib/constants';
	import Switch from '$lib/components/common/Switch.svelte';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	let engine = 'searxng';
	let url = '';
	let apiKey = '';
	let cx = '';
	let count = 5;
	let fetchContent = true;
	let preferNative = false;
	let testing = false;
	let results: { title: string; url: string }[] = [];

	const engines = [
		['searxng', $i18n.t('SearXNG (recommended)')],
		['tavily', 'Tavily'],
		['brave', 'Brave'],
		['bing', 'Bing'],
		['google', 'Google'],
		['duckduckgo', 'DuckDuckGo'],
		['native', $i18n.t('Model built-in search')]
	];

	const basePlaceholder = {
		searxng: 'https://search.hyw.mom',
		tavily: 'https://api.tavily.com',
		brave: 'https://api.search.brave.com',
		bing: 'https://api.bing.microsoft.com',
		google: 'https://www.googleapis.com',
		duckduckgo: 'https://html.duckduckgo.com'
	};

	async function load() {
		const res = await fetch(`${WEBUI_API_BASE_URL}/configs/web_search`, {
			headers: { Authorization: `Bearer ${localStorage.token}` }
		});
		if (!res.ok) return;
		const data = await res.json();
		engine = data.engine || 'searxng';
		url = data.url || '';
		apiKey = data.api_key || '';
		cx = data.cx || '';
		count = data.count || 5;
		fetchContent = data.fetch_content ?? true;
		preferNative = data.prefer_native ?? false;
	}

	async function save() {
		const res = await fetch(`${WEBUI_API_BASE_URL}/configs/web_search`, {
			method: 'POST',
			headers: {
				Authorization: `Bearer ${localStorage.token}`,
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({
				engine,
				url,
				api_key: apiKey,
				cx,
				count,
				fetch_content: fetchContent,
				prefer_native: preferNative
			})
		});
		if (!res.ok) {
			toast.error($i18n.t('Failed to save'));
			return;
		}
		toast.success($i18n.t('Saved'));
	}

	async function testSearch() {
		testing = true;
		results = [];
		try {
			const res = await fetch(`${WEBUI_API_BASE_URL}/configs/web_search/test`, {
				method: 'POST',
				headers: {
					Authorization: `Bearer ${localStorage.token}`,
					'Content-Type': 'application/json'
				},
				body: JSON.stringify({ engine, url, api_key: apiKey, cx, query: 'open models' })
			});
			const data = await res.json().catch(() => ({}));
			if (!res.ok) {
				toast.error(data.detail || $i18n.t('Search failed'));
				return;
			}
			results = data.results || [];
			if (!results.length) {
				toast.error($i18n.t('No search results'));
			} else {
				toast.success($i18n.t('Found {{count}} results', { count: results.length }));
			}
		} catch (error) {
			toast.error(String(error));
		} finally {
			testing = false;
		}
	}

	onMount(load);
</script>

<div class="flex flex-col gap-3 text-sm max-w-xl">
	<div class="text-sm font-medium">{$i18n.t('Web Search')}</div>
	<div class="text-xs text-gray-500">
		{$i18n.t(
			'SearXNG is the recommended engine. Enter only the site root, such as https://search.hyw.mom. The request path is added automatically. The same applies to the other engines. Built-in model search is used when the model has the web capability; otherwise the engine below is used.'
		)}
	</div>
	<label class="flex flex-col gap-1">
		<span>{$i18n.t('Engine')}</span>
		<select class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={engine}>
			{#each engines as [id, label]}
				<option value={id}>{label}</option>
			{/each}
		</select>
	</label>
	{#if engine !== 'native'}
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Base URL')}</span>
			<input
				class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2"
				bind:value={url}
				placeholder={basePlaceholder[engine] || ''}
			/>
		</label>
	{/if}
	{#if engine === 'tavily' || engine === 'brave' || engine === 'bing' || engine === 'google'}
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('API Key')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" type="password" bind:value={apiKey} />
		</label>
	{/if}
	{#if engine === 'google'}
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Search engine id')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={cx} />
		</label>
	{/if}
	<label class="flex flex-col gap-1">
		<span>{$i18n.t('Result count')}</span>
		<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2 w-24" type="number" min="1" max="8" bind:value={count} />
	</label>
	<div class="flex items-center justify-between">
		<span>{$i18n.t('Fetch page text')}</span>
		<Switch bind:state={fetchContent} />
	</div>
	<div class="flex items-center justify-between">
		<span>{$i18n.t('Prefer the model built-in search')}</span>
		<Switch bind:state={preferNative} />
	</div>
	<div class="flex gap-2">
		<button type="button" class="px-3 py-1.5 rounded-lg bg-gray-900 text-white dark:bg-white dark:text-black" on:click={save}>
			{$i18n.t('Save')}
		</button>
		<button
			type="button"
			class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 disabled:opacity-50"
			on:click={testSearch}
			disabled={testing || engine === 'native'}
		>
			{testing ? $i18n.t('Testing') : $i18n.t('Test')}
		</button>
	</div>
	{#if results.length}
		<ul class="text-xs space-y-1">
			{#each results as item}
				<li><a class="underline" href={item.url} target="_blank" rel="noreferrer">{item.title}</a></li>
			{/each}
		</ul>
	{/if}
</div>
