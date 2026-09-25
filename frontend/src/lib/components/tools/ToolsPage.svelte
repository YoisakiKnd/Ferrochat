<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { models, showSidebar } from '$lib/stores';
	import { toast } from 'svelte-sonner';
	import MenuLines from '$lib/components/icons/MenuLines.svelte';
	import Select from '$lib/components/common/Select.svelte';
	import Toggle from '$lib/components/common/Toggle.svelte';
	import SegmentedTabs from '$lib/components/common/SegmentedTabs.svelte';
	import Selector from '$lib/components/chat/ModelSelector/Selector.svelte';
	import Markdown from '$lib/components/chat/Messages/Markdown.svelte';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	const tabs = ['translate', 'polish', 'summary'] as const;
	type Tab = (typeof tabs)[number];

	let tab: Tab = 'translate';
	let source = '';
	let result = '';
	let sources: { name?: string; url?: string }[] = [];
	let modelId = '';
	let sourceLang = 'auto';
	let targetLang = '简体中文';
	let tone = 'natural';
	let length = 'short';
	let webSearch = true;
	let running = false;
	let controller: AbortController | null = null;
	let history: { tab: Tab; source: string; result: string; at: number }[] = [];

	const languages = ['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Français', 'Deutsch', 'Español'];

	onMount(() => {
		const saved = localStorage.getItem('ferrochat-tool-history');
		if (saved) {
			try {
				history = JSON.parse(saved);
			} catch {
				history = [];
			}
		}
		modelId = localStorage.getItem('ferrochat-tool-model') ?? '';
		const query = $page.url.searchParams.get('tab');
		if (query === 'translate' || query === 'polish' || query === 'summary') tab = query;
	});

	$: usableModels = $models.filter((model) => String(model.id).includes(':'));
	$: modelItems = usableModels.map((model) => ({
		label: model.name ?? model.id,
		value: model.id,
		model
	}));
	$: if (!modelId && usableModels.length > 0) modelId = usableModels[0].id;
	$: if (modelId) localStorage.setItem('ferrochat-tool-model', modelId);

	const setTab = (next: Tab) => {
		tab = next;
		result = '';
		sources = [];
		goto(`/tools?tab=${next}`, { replaceState: true, keepFocus: true, noScroll: true });
	};

	const promptFor = () => {
		if (tab === 'polish') {
			const toneText = tone === 'formal' ? 'formal' : tone === 'brief' ? 'concise' : 'natural';
			return `Rewrite the user text in a ${toneText} tone. Do not add facts. Reply with the revised text only.`;
		}
		if (tab === 'summary') {
			const size = length === 'long' ? 'several paragraphs' : length === 'medium' ? 'a short paragraph' : 'a few bullets';
			return `Summarize the user text as ${size}.`;
		}
		const from = sourceLang === 'auto' ? 'Detect the source language.' : `The source language is ${sourceLang}.`;
		return `${from} Translate the user text into ${targetLang}. Keep the meaning and tone. Reply with the translation only.`;
	};

	const saveHistory = () => {
		history = [{ tab, source, result, at: Date.now() }, ...history].slice(0, 20);
		localStorage.setItem('ferrochat-tool-history', JSON.stringify(history));
	};

	const deleteHistory = (at: number) => {
		history = history.filter((item) => item.at !== at);
		localStorage.setItem('ferrochat-tool-history', JSON.stringify(history));
	};

	const stop = () => {
		controller?.abort();
		running = false;
	};

	const run = async () => {
		const text = source.trim();
		if (!text) return;
		if (!modelId) {
			toast.error($i18n.t('Please select a model first.'));
			return;
		}
		running = true;
		result = '';
		sources = [];
		controller = new AbortController();
		try {
			const response = await fetch('/api/chat/completions', {
				method: 'POST',
				headers: {
					Authorization: `Bearer ${localStorage.token}`,
					'Content-Type': 'application/json'
				},
				signal: controller.signal,
				body: JSON.stringify({
					stream: true,
					model: modelId,
					messages: [
						{ role: 'system', content: promptFor() },
						{ role: 'user', content: text }
					],
					features: { web_search: tab === 'summary' && webSearch }
				})
			});
			if (!response.ok || !response.body) {
				const detail = await response.json().catch(() => ({}));
				throw new Error(detail.detail || detail.error || response.statusText);
			}
			const reader = response.body.getReader();
			const decoder = new TextDecoder();
			let buffer = '';
			while (true) {
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				const parts = buffer.split('\n\n');
				buffer = parts.pop() ?? '';
				for (const part of parts) {
					const line = part.split('\n').find((item) => item.startsWith('data: '));
					if (!line || line.includes('[DONE]')) continue;
					const payload = JSON.parse(line.slice(6));
					if (payload.error) throw new Error(payload.error);
					if (payload.sources) sources = payload.sources;
					const delta = payload.choices?.[0]?.delta?.content;
					if (delta) result += delta;
				}
			}
			if (result) saveHistory();
		} catch (error) {
			if ((error as Error).name !== 'AbortError') toast.error(String(error));
		} finally {
			running = false;
			controller = null;
		}
	};

	const copyResult = async () => {
		if (!result) return;
		await navigator.clipboard.writeText(result);
		toast.success($i18n.t('Copied'));
	};
</script>

<div class="flex flex-col h-full w-full">
	<div class="flex items-center gap-2 px-3 py-2 border-b border-gray-100 dark:border-gray-850">
		<button
			id="sidebar-toggle-button"
			class="{$showSidebar ? 'md:hidden' : ''} cursor-pointer px-2 py-2 flex rounded-xl hover:bg-gray-50 dark:hover:bg-gray-850"
			type="button"
			aria-label="Toggle Sidebar"
			on:click={() => showSidebar.set(!$showSidebar)}
		>
			<MenuLines />
		</button>
		<SegmentedTabs
			value={tab}
			options={[
				{ value: 'translate', label: $i18n.t('Translate') },
				{ value: 'polish', label: $i18n.t('Polish') },
				{ value: 'summary', label: $i18n.t('Summarize') }
			]}
			on:change={(event) => setTab(event.detail)}
		/>
		<div class="ml-auto min-w-40 max-w-full">
			{#if modelItems.length === 0}
				<a class="text-sm underline" href="/admin/settings">{$i18n.t('Add a provider')}</a>
			{:else}
				<Selector bind:value={modelId} items={modelItems} placeholder={$i18n.t('Select a model')} className="w-full" triggerClassName="text-sm" />
			{/if}
		</div>
	</div>

	<div class="flex-1 grid grid-cols-1 md:grid-cols-2 gap-4 p-4 min-h-0">
		<div class="flex flex-col gap-3 min-h-0">
			{#if tab === 'translate'}
				<div class="flex items-center gap-2 text-sm">
					<Select
						bind:value={sourceLang}
						options={[{ value: 'auto', label: $i18n.t('Auto') }, ...languages.map((language) => ({ value: language, label: language }))]}
					/>
					<button
						type="button"
						class="px-2 py-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800"
						on:click={() => {
							if (sourceLang === 'auto') return;
							const next = sourceLang;
							sourceLang = targetLang;
							targetLang = next;
						}}
					>
						{$i18n.t('Swap')}
					</button>
					<Select
						bind:value={targetLang}
						options={languages.map((language) => ({ value: language, label: language }))}
					/>
				</div>
			{:else if tab === 'polish'}
				<Select
					className="self-start"
					bind:value={tone}
					options={[
						{ value: 'natural', label: $i18n.t('Natural') },
						{ value: 'formal', label: $i18n.t('Formal') },
						{ value: 'brief', label: $i18n.t('Brief') }
					]}
				/>
			{:else}
				<div class="flex items-center gap-3 text-sm">
					<Select
						bind:value={length}
						options={[
							{ value: 'short', label: $i18n.t('Short') },
							{ value: 'medium', label: $i18n.t('Medium') },
							{ value: 'long', label: $i18n.t('Long') }
						]}
					/>
					<Toggle bind:checked={webSearch} label={$i18n.t('Web Search')} />
				</div>
			{/if}
			<textarea
				class="flex-1 min-h-48 rounded-xl border border-gray-200 dark:border-gray-800 bg-transparent px-3 py-2"
				placeholder={$i18n.t('Paste text here')}
				bind:value={source}
			></textarea>
			<div class="flex gap-2">
				<button
					type="button"
					class="px-4 py-2 rounded-lg bg-gray-900 text-white dark:bg-white dark:text-gray-900 disabled:opacity-50"
					disabled={running || !source.trim()}
					on:click={run}
				>
					{$i18n.t(tab === 'translate' ? 'Translate' : tab === 'polish' ? 'Polish' : 'Summarize')}
				</button>
				{#if running}
					<button type="button" class="px-4 py-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800" on:click={stop}>
						{$i18n.t('Stop')}
					</button>
				{/if}
			</div>
		</div>
		<div class="flex flex-col gap-3 min-h-0">
			<div class="flex-1 min-h-48 rounded-xl bg-gray-50 dark:bg-gray-900 px-4 py-3 overflow-auto">
				{#if result}
					<Markdown content={result} />
				{/if}
			</div>
			<button type="button" class="self-start px-3 py-1.5 text-sm rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800" on:click={copyResult}>
				{$i18n.t('Copy')}
			</button>
			{#if sources.length}
				<div class="grid gap-2">
					{#each sources as sourceItem, index}
						<a
							class="block rounded-lg border border-gray-200 dark:border-gray-800 px-3 py-2 text-sm hover:bg-gray-50 dark:hover:bg-gray-850"
							href={sourceItem.url}
							target="_blank"
							rel="noreferrer"
						>
							<div class="font-medium">[{index + 1}] {sourceItem.name || sourceItem.url}</div>
							<div class="text-xs text-gray-500 truncate">{sourceItem.url}</div>
						</a>
					{/each}
				</div>
			{/if}
			{#if history.filter((item) => item.tab === tab).length}
				<div class="text-sm text-gray-500">{$i18n.t('Recent')}</div>
				<div class="flex flex-col gap-1 max-h-32 overflow-auto">
					{#each history.filter((item) => item.tab === tab) as item}
						<div class="flex items-center gap-2 rounded px-2 py-1 hover:bg-gray-50 dark:hover:bg-gray-850">
							<button
								type="button"
								class="min-w-0 flex-1 text-left text-sm"
								on:click={() => {
									source = item.source;
									result = item.result;
								}}
							>
								<div class="truncate">{item.source}</div>
								<div class="text-xs text-gray-500">
									{$i18n.t(item.tab === 'translate' ? 'Translate' : item.tab === 'polish' ? 'Polish' : 'Summarize')}
									· {new Date(item.at).toLocaleString()}
								</div>
							</button>
							<button
								type="button"
								class="text-xs text-gray-500"
								on:click={() => deleteHistory(item.at)}
							>
								{$i18n.t('Delete')}
							</button>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</div>
