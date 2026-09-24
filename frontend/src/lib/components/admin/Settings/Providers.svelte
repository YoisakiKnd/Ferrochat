<script lang="ts">
	import { onMount, getContext, createEventDispatcher } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { WEBUI_API_BASE_URL } from '$lib/constants';
	import { models as modelsStore } from '$lib/stores';
	import { getModels } from '$lib/apis';
	import Modal from '$lib/components/common/Modal.svelte';
	import Spinner from '$lib/components/common/Spinner.svelte';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');
	const dispatch = createEventDispatcher();

	type Provider = {
		id: string;
		type: string;
		name: string;
		base_url: string;
		api_keys: string;
		headers: Record<string, string>;
		enabled: boolean;
		sort_order: number;
		is_builtin: boolean;
	};

	type RemoteModel = {
		id: string;
		name: string;
		group: string;
		capabilities: Record<string, boolean>;
	};

	const TYPES = ['openai', 'anthropic', 'gemini', 'ollama', 'azure'];
	const CAPS = ['vision', 'reasoning', 'tools', 'web', 'embedding'];

	let providers: Provider[] = [];
	let selected: Provider | null = null;
	let addedIds = new Set<string>();
	let keyHealth = [];
	let showKey = false;
	let importInput;
	let manualId = '';
	let checking = false;

	let showFetch = false;
	let fetching = false;
	let remote: RemoteModel[] = [];
	let picked = new Set<string>();
	let filter = '';
	let adding = false;

	const authHeaders = () => ({
		Authorization: `Bearer ${localStorage.token}`,
		'Content-Type': 'application/json'
	});

	const api = async (path: string, init: RequestInit = {}) => {
		const res = await fetch(`${WEBUI_API_BASE_URL}${path}`, { headers: authHeaders(), ...init });
		const body = await res.json().catch(() => null);
		if (!res.ok) throw new Error(body?.detail ?? res.statusText);
		return body;
	};

	const refreshChatModels = async () => {
		modelsStore.set(await getModels(localStorage.token));
	};

	const load = async () => {
		providers = await api('/providers');
		const current = providers.find((p) => p.id === selected?.id) ?? providers[0];
		if (current) await select(current);
	};

	const select = async (provider: Provider) => {
		selected = { ...provider, headers: provider.headers ?? {} };
		showKey = false;
		const [list, health] = await Promise.all([
			api(`/providers/${provider.id}/models`).catch(() => []),
			api(`/providers/${provider.id}/keys`).catch(() => [])
		]);
		addedIds = new Set(list.map((m) => m.model_id));
		keyHealth = health;
	};

	const save = async () => {
		if (!selected) return;
		try {
			await api('/providers', { method: 'POST', body: JSON.stringify(selected) });
			toast.success($i18n.t('Saved'));
			await load();
			await refreshChatModels();
		} catch (e) {
			toast.error(`${e}`);
		}
	};

	const check = async () => {
		if (!selected) return;
		checking = true;
		try {
			await api('/providers', { method: 'POST', body: JSON.stringify(selected) });
			await api(`/providers/${selected.id}/check`, {
				method: 'POST',
				body: JSON.stringify({ model: [...addedIds][0] })
			});
			toast.success($i18n.t('Connection successful'));
		} catch (e) {
			toast.error(`${$i18n.t('Connection failed')}: ${e.message}`);
		}
		checking = false;
	};

	const exportConfig = async () => {
		const data = await api('/providers/export');
		const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = 'ferrochat-providers.json';
		a.click();
		URL.revokeObjectURL(url);
	};

	const importConfig = async (event) => {
		const file = event.target.files?.[0];
		if (!file) return;
		try {
			await api('/providers/import', { method: 'POST', body: await file.text() });
			toast.success($i18n.t('Imported'));
			await load();
		} catch (e) {
			toast.error(`${e}`);
		}
		event.target.value = '';
	};

	const addCustom = async () => {
		const created = await api('/providers', {
			method: 'POST',
			body: JSON.stringify({
				id: `custom-${Date.now()}`,
				type: 'openai',
				name: $i18n.t('Custom Provider'),
				base_url: 'https://api.example.com/v1',
				api_keys: '',
				headers: {},
				enabled: true,
				sort_order: providers.length
			})
		});
		selected = created;
		await load();
	};

	const removeProvider = async () => {
		if (!selected || selected.is_builtin) return;
		if (!confirm($i18n.t('Delete this provider and all of its models?'))) return;
		await api(`/providers/${selected.id}`, { method: 'DELETE' });
		selected = null;
		await load();
		await refreshChatModels();
	};

	const openFetch = async () => {
		if (!selected) return;
		showFetch = true;
		fetching = true;
		remote = [];
		picked = new Set();
		filter = '';
		try {
			await api('/providers', { method: 'POST', body: JSON.stringify(selected) });
			remote = await api(`/providers/${selected.id}/remote-models`);
		} catch (e) {
			toast.error(`${$i18n.t('Failed to load models')}: ${e.message}`);
		}
		fetching = false;
	};

	const togglePick = (id: string) => {
		picked.has(id) ? picked.delete(id) : picked.add(id);
		picked = picked;
	};

	const toggleGroup = (items: RemoteModel[]) => {
		const open = items.filter((m) => !addedIds.has(m.id));
		const all = open.every((m) => picked.has(m.id));
		open.forEach((m) => (all ? picked.delete(m.id) : picked.add(m.id)));
		picked = picked;
	};

	const addPicked = async () => {
		if (!selected || picked.size === 0) return;
		adding = true;
		try {
			const models = remote
				.filter((m) => picked.has(m.id))
				.map((m) => ({ model_id: m.id, name: m.name, capabilities: m.capabilities }));
			const added = await api(`/providers/${selected.id}/models/batch`, {
				method: 'POST',
				body: JSON.stringify({ models })
			});
			toast.success($i18n.t('Added {{COUNT}} models', { COUNT: added.length }));
			picked = new Set();
			await select(selected);
			await refreshChatModels();
		} catch (e) {
			toast.error(`${e}`);
		}
		adding = false;
	};

	const addManual = async () => {
		const id = manualId.trim();
		if (!selected || !id) return;
		try {
			await api(`/providers/${selected.id}/models/batch`, {
				method: 'POST',
				body: JSON.stringify({ models: [{ model_id: id, name: id }] })
			});
			manualId = '';
			toast.success($i18n.t('Added {{COUNT}} models', { COUNT: 1 }));
			await select(selected);
			await refreshChatModels();
		} catch (e) {
			toast.error(`${e}`);
		}
	};

	$: visible = remote.filter(
		(m) =>
			!filter ||
			m.id.toLowerCase().includes(filter.toLowerCase()) ||
			m.name.toLowerCase().includes(filter.toLowerCase())
	);
	$: remoteGroups = visible.reduce((acc: Record<string, RemoteModel[]>, model) => {
		const key = model.group || 'other';
		(acc[key] = acc[key] ?? []).push(model);
		return acc;
	}, {});
	$: endpoint = selected
		? selected.type === 'anthropic'
			? `${selected.base_url.replace(/\/$/, '')}/v1/messages`
			: selected.type === 'gemini'
				? `${selected.base_url.replace(/\/$/, '')}/models/{model}:streamGenerateContent`
				: `${selected.base_url.replace(/\/$/, '')}/chat/completions`
		: '';

	onMount(load);
</script>

<div class="flex flex-col lg:flex-row gap-4 min-h-[70vh]">
	<aside class="lg:w-60 shrink-0 space-y-1">
		<div class="flex items-center justify-between pb-1">
			<div class="text-sm font-medium">{$i18n.t('Providers')}</div>
			<button
				class="text-xs px-2 py-1 rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700"
				on:click={addCustom}>+ {$i18n.t('Add')}</button
			>
		</div>
		{#each providers as provider (provider.id)}
			<button
				class="w-full text-left px-3 py-2 rounded-xl flex items-center justify-between text-sm transition {selected?.id ===
				provider.id
					? 'bg-gray-100 dark:bg-gray-800'
					: 'hover:bg-gray-50 dark:hover:bg-gray-850'}"
				on:click={() => select(provider)}
			>
				<span class="truncate">{provider.name}</span>
				<span
					class="size-2 rounded-full shrink-0 {provider.enabled ? 'bg-green-500' : 'bg-gray-300 dark:bg-gray-600'}"
				/>
			</button>
		{/each}
	</aside>

	{#if selected}
		<section class="flex-1 space-y-3 min-w-0">
			<div class="flex items-center justify-between gap-3">
				<input
					class="text-lg font-medium bg-transparent outline-none flex-1 min-w-0"
					bind:value={selected.name}
				/>
				<label class="text-sm flex items-center gap-2 shrink-0">
					<input type="checkbox" bind:checked={selected.enabled} />
					{$i18n.t('Enabled')}
				</label>
			</div>

			{#if !selected.is_builtin}
				<div>
					<label class="block text-xs text-gray-500 mb-1" for="provider-type">{$i18n.t('Type')}</label>
					<select
						id="provider-type"
						class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
						bind:value={selected.type}
					>
						{#each [...new Set([...TYPES, selected.type])] as t}
							<option value={t}>{t}</option>
						{/each}
					</select>
				</div>
			{/if}

			<div>
				<label class="block text-xs text-gray-500 mb-1" for="provider-key">API Key</label>
				<div class="flex gap-2">
					{#if showKey}
						<input
							id="provider-key"
							class="flex-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
							bind:value={selected.api_keys}
							placeholder="sk-..., sk-..."
						/>
					{:else}
						<input
							id="provider-key"
							type="password"
							class="flex-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
							bind:value={selected.api_keys}
							placeholder="sk-..., sk-..."
						/>
					{/if}
					<button
						class="px-3 rounded-lg bg-gray-100 dark:bg-gray-800 text-xs"
						on:click={() => (showKey = !showKey)}>{showKey ? $i18n.t('Hide') : $i18n.t('Show')}</button
					>
				</div>
				<div class="text-xs text-gray-400 mt-1">
					{$i18n.t('Separate multiple keys with commas; they are rotated automatically.')}
				</div>
			</div>

			<div>
				<label class="block text-xs text-gray-500 mb-1" for="provider-base">API Base URL</label>
				<input
					id="provider-base"
					class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
					bind:value={selected.base_url}
				/>
				<div class="text-xs text-gray-400 mt-1 break-all">{endpoint}</div>
			</div>

			{#if keyHealth.length}
				<div class="text-xs space-y-0.5">
					{#each keyHealth as key}
						<div class="text-amber-600">
							{key.fingerprint}: {key.last_error || 'ok'}
						</div>
					{/each}
				</div>
			{/if}

			<div class="flex gap-2 flex-wrap">
				<button
					class="px-3 py-1.5 rounded-lg bg-black text-white text-sm dark:bg-white dark:text-black"
					on:click={save}>{$i18n.t('Save')}</button
				>
				<button
					class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm flex items-center gap-1"
					disabled={checking}
					on:click={check}
				>
					{#if checking}<Spinner className="size-3" />{/if}
					{$i18n.t('Check')}
				</button>
				<button class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm" on:click={exportConfig}
					>{$i18n.t('Export')}</button
				>
				<button
					class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm"
					on:click={() => importInput.click()}>{$i18n.t('Import')}</button
				>
				<input
					bind:this={importInput}
					type="file"
					accept="application/json"
					class="hidden"
					on:change={importConfig}
				/>
				{#if !selected.is_builtin}
					<button class="px-3 py-1.5 rounded-lg text-sm text-red-500" on:click={removeProvider}
						>{$i18n.t('Delete')}</button
					>
				{/if}
			</div>

			<div class="rounded-xl border border-gray-100 dark:border-gray-800 p-3 space-y-3">
				<div class="flex items-center justify-between gap-2">
					<div class="text-sm">
						<span class="font-medium">{$i18n.t('Models')}</span>
						<span class="text-gray-500 ml-1">{$i18n.t('{{COUNT}} added', { COUNT: addedIds.size })}</span>
					</div>
					<div class="flex gap-2">
						<button
							class="text-sm px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800"
							on:click={() => dispatch('models')}>{$i18n.t('Manage Models')}</button
						>
						<button
							class="text-sm px-3 py-1.5 rounded-lg bg-black text-white dark:bg-white dark:text-black"
							on:click={openFetch}>{$i18n.t('Fetch Models')}</button
						>
					</div>
				</div>
				<div class="flex gap-2">
					<input
						class="flex-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
						bind:value={manualId}
						placeholder={$i18n.t('Add a model ID manually')}
						on:keydown={(e) => e.key === 'Enter' && addManual()}
					/>
					<button class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm" on:click={addManual}
						>{$i18n.t('Add')}</button
					>
				</div>
			</div>
		</section>
	{/if}
</div>

<Modal size="md" bind:show={showFetch}>
	<div class="p-5 space-y-3" data-testid="fetch-models">
		<div class="flex items-center justify-between">
			<div class="text-lg font-medium">{selected?.name} · {$i18n.t('Fetch Models')}</div>
			<button class="text-sm text-gray-500" on:click={() => (showFetch = false)}>✕</button>
		</div>
		<input
			class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
			bind:value={filter}
			placeholder={$i18n.t('Search Models')}
		/>
		<div class="max-h-[55vh] overflow-y-auto space-y-3 pr-1">
			{#if fetching}
				<div class="flex justify-center py-8"><Spinner className="size-5" /></div>
			{:else if remote.length === 0}
				<div class="text-sm text-gray-500 py-6 text-center">{$i18n.t('No models found')}</div>
			{:else}
				{#each Object.entries(remoteGroups) as [group, items] (group)}
					<div>
						<div class="flex items-center justify-between text-xs font-medium text-gray-500 pb-1">
							<span>{group} ({items.length})</span>
							<button class="hover:underline" on:click={() => toggleGroup(items)}
								>{$i18n.t('Select all')}</button
							>
						</div>
						{#each items as model (model.id)}
							{@const done = addedIds.has(model.id)}
							<label
								class="flex items-center gap-2 text-sm py-1 px-2 rounded-lg {done
									? 'opacity-60'
									: 'hover:bg-gray-50 dark:hover:bg-gray-850 cursor-pointer'}"
							>
								<input
									type="checkbox"
									disabled={done}
									checked={done || picked.has(model.id)}
									on:change={() => togglePick(model.id)}
								/>
								<span class="flex-1 truncate">{model.name}</span>
								{#each CAPS.filter((c) => model.capabilities?.[c]) as cap}
									<span class="text-[10px] px-1.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-500"
										>{$i18n.t(cap)}</span
									>
								{/each}
								{#if done}
									<span class="text-xs text-green-600">{$i18n.t('Added')}</span>
								{/if}
							</label>
						{/each}
					</div>
				{/each}
			{/if}
		</div>
		<div class="flex items-center justify-between pt-2">
			<button
				class="text-sm text-gray-500 hover:underline"
				on:click={() => {
					visible.filter((m) => !addedIds.has(m.id)).forEach((m) => picked.add(m.id));
					picked = picked;
				}}>{$i18n.t('Select all')}</button
			>
			<button
				class="px-4 py-2 rounded-lg bg-black text-white text-sm dark:bg-white dark:text-black disabled:opacity-40 flex items-center gap-1"
				disabled={picked.size === 0 || adding}
				on:click={addPicked}
			>
				{#if adding}<Spinner className="size-3" />{/if}
				{$i18n.t('Add selected ({{COUNT}})', { COUNT: picked.size })}
			</button>
		</div>
	</div>
</Modal>
