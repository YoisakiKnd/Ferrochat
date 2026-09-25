<script lang="ts">
	import { onMount, getContext, createEventDispatcher } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { WEBUI_API_BASE_URL } from '$lib/constants';
	import { config, models as modelsStore } from '$lib/stores';
	import { getBackendConfig, getModels } from '$lib/apis';
	import Modal from '$lib/components/common/Modal.svelte';
	import Spinner from '$lib/components/common/Spinner.svelte';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');
	const dispatch = createEventDispatcher();

	type ManagedModel = {
		id: string;
		provider_id: string;
		provider_name: string;
		provider_enabled: boolean;
		model_id: string;
		name: string;
		capabilities: Record<string, boolean>;
		params: Record<string, any>;
		enabled: boolean;
		sort_order: number;
	};

	const CAPS = ['vision', 'reasoning', 'tools', 'web', 'embedding', 'audio'];
	const EFFORTS = ['', 'low', 'medium', 'high'];

	let loaded = false;
	let items: ManagedModel[] = [];
	let defaultModel = '';
	let query = '';
	let providerFilter = '';
	let capFilter = '';
	let dragId: string | null = null;

	let editing: ManagedModel | null = null;
	let showEdit = false;
	let form = {
		name: '',
		capabilities: {} as Record<string, boolean>,
		temperature: '',
		top_p: '',
		max_tokens: '',
		reasoning_effort: '',
		system: '',
		input_price: '' as string | number,
		output_price: '' as string | number
	};

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

	const load = async () => {
		const data = await api('/models/managed');
		items = data.models;
		defaultModel = typeof data.default_model === 'string' ? data.default_model : '';
		loaded = true;
	};

	const syncChat = async () => {
		modelsStore.set(await getModels(localStorage.token));
	};

	const patch = async (model: ManagedModel, body: Record<string, unknown>) => {
		try {
			const updated = await api(
				`/providers/${model.provider_id}/models/${encodeURIComponent(model.model_id)}`,
				{ method: 'POST', body: JSON.stringify(body) }
			);
			items = items.map((m) => (m.id === model.id ? { ...m, ...updated } : m));
			await syncChat();
		} catch (e) {
			toast.error(`${e}`);
		}
	};

	const remove = async (model: ManagedModel) => {
		if (!confirm($i18n.t('Remove {{NAME}} from the model list?', { NAME: model.name }))) return;
		await api(`/providers/${model.provider_id}/models/${encodeURIComponent(model.model_id)}`, {
			method: 'DELETE'
		});
		items = items.filter((m) => m.id !== model.id);
		await syncChat();
	};

	const setDefault = async (model: ManagedModel) => {
		const next = defaultModel === model.id ? '' : model.id;
		await api('/models/managed/default', {
			method: 'POST',
			body: JSON.stringify({ default_model: next })
		});
		defaultModel = next;
		config.set(await getBackendConfig());
	};

	const setAll = async (enabled: boolean) => {
		for (const model of filtered.filter((m) => m.enabled !== enabled)) {
			await api(`/providers/${model.provider_id}/models/${encodeURIComponent(model.model_id)}`, {
				method: 'POST',
				body: JSON.stringify({ enabled })
			});
		}
		await load();
		await syncChat();
	};

	const drop = async (targetId: string) => {
		if (!dragId || dragId === targetId) return;
		const from = items.findIndex((m) => m.id === dragId);
		const to = items.findIndex((m) => m.id === targetId);
		const next = [...items];
		const [moved] = next.splice(from, 1);
		next.splice(to, 0, moved);
		items = next;
		dragId = null;
		await api('/models/managed/reorder', {
			method: 'POST',
			body: JSON.stringify({ ids: items.map((m) => m.id) })
		});
		await syncChat();
	};

	const openEdit = (model: ManagedModel) => {
		editing = model;
		const p = model.params ?? {};
		form = {
			name: model.name,
			capabilities: { ...model.capabilities },
			temperature: p.temperature ?? '',
			top_p: p.top_p ?? '',
			max_tokens: p.max_tokens ?? '',
			reasoning_effort: p.reasoning_effort ?? '',
			system: p.system ?? '',
			input_price: p.input_price ?? '',
			output_price: p.output_price ?? ''
		};
		showEdit = true;
	};

	const saveEdit = async () => {
		if (!editing) return;
		const params: Record<string, any> = {};
		for (const key of ['temperature', 'top_p', 'max_tokens']) {
			const raw = `${form[key]}`.trim();
			if (raw !== '') params[key] = Number(raw);
		}
		if (form.reasoning_effort) params.reasoning_effort = form.reasoning_effort;
		if (form.system.trim()) params.system = form.system;
		for (const key of ['input_price', 'output_price']) {
			const raw = `${form[key]}`.trim();
			if (raw !== '') params[key] = Number(raw);
		}
		await patch(editing, {
			name: form.name.trim() || editing.model_id,
			capabilities: form.capabilities,
			params
		});
		showEdit = false;
		toast.success($i18n.t('Saved'));
	};

	$: providers = [...new Map(items.map((m) => [m.provider_id, m.provider_name])).entries()];
	$: filtered = items.filter(
		(m) =>
			(!query ||
				m.name.toLowerCase().includes(query.toLowerCase()) ||
				m.model_id.toLowerCase().includes(query.toLowerCase())) &&
			(!providerFilter || m.provider_id === providerFilter) &&
			(!capFilter || m.capabilities?.[capFilter])
	);
	$: canDrag = !query && !providerFilter && !capFilter;
	$: enabledCount = items.filter((m) => m.enabled).length;

	onMount(load);
</script>

<div class="space-y-3" data-testid="models-admin">
	<div class="flex items-center justify-between gap-2 flex-wrap">
		<div class="text-lg font-medium">
			{$i18n.t('Models')}
			<span class="text-sm text-gray-500 font-normal ml-1"
				>{$i18n.t('{{ENABLED}} of {{TOTAL}} enabled', { ENABLED: enabledCount, TOTAL: items.length })}</span
			>
		</div>
		<button
			class="text-sm px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800"
			on:click={() => dispatch('providers')}>+ {$i18n.t('Add from providers')}</button
		>
	</div>

	<div class="flex gap-2 flex-wrap">
		<input
			class="flex-1 min-w-40 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
			bind:value={query}
			placeholder={$i18n.t('Search Models')}
		/>
		<select class="rounded-lg pl-3 pr-8 py-2 text-sm bg-gray-50 dark:bg-gray-900" bind:value={providerFilter}>
			<option value="">{$i18n.t('All providers')}</option>
			{#each providers as [id, name]}
				<option value={id}>{name}</option>
			{/each}
		</select>
		<select class="rounded-lg pl-3 pr-8 py-2 text-sm bg-gray-50 dark:bg-gray-900" bind:value={capFilter}>
			<option value="">{$i18n.t('All capabilities')}</option>
			{#each CAPS as cap}
				<option value={cap}>{$i18n.t(cap)}</option>
			{/each}
		</select>
	</div>

	<div class="flex items-center justify-between text-xs text-gray-500">
		<span>{canDrag ? $i18n.t('Drag to reorder; the chat model selector uses this order.') : ''}</span>
		<span class="flex gap-3">
			<button class="hover:underline" on:click={() => setAll(true)}>{$i18n.t('Enable all')}</button>
			<button class="hover:underline" on:click={() => setAll(false)}>{$i18n.t('Disable all')}</button>
		</span>
	</div>

	{#if !loaded}
		<div class="flex justify-center py-10"><Spinner /></div>
	{:else if items.length === 0}
		<div class="text-sm text-gray-500 text-center py-10 space-y-2">
			<div>{$i18n.t('No models yet. Fetch models from a provider and pick the ones you want.')}</div>
			<button class="underline" on:click={() => dispatch('providers')}>{$i18n.t('Go to Providers')}</button>
		</div>
	{:else}
		<div class="divide-y divide-gray-100 dark:divide-gray-850">
			{#each filtered as model (model.id)}
				<div
					class="flex items-center gap-3 py-2 px-1 {dragId === model.id ? 'opacity-40' : ''}"
					draggable={canDrag}
					on:dragstart={() => (dragId = model.id)}
					on:dragend={() => (dragId = null)}
					on:dragover|preventDefault
					on:drop|preventDefault={() => drop(model.id)}
					role="listitem"
				>
					{#if canDrag}
						<span class="cursor-grab text-gray-400 select-none">⋮⋮</span>
					{/if}
					<button class="flex-1 min-w-0 text-left" on:click={() => openEdit(model)}>
						<div class="text-sm truncate {model.enabled ? '' : 'text-gray-400'}">
							{model.name}
							{#if defaultModel === model.id}
								<span class="text-[10px] ml-1 px-1.5 rounded bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300"
									>{$i18n.t('Default')}</span
								>
							{/if}
						</div>
						<div class="text-xs text-gray-500 truncate">
							{model.provider_name}{model.provider_enabled ? '' : ` (${$i18n.t('disabled')})`} · {model.model_id}
						</div>
					</button>
					<div class="hidden md:flex gap-1">
						{#each CAPS as cap}
							<button
								class="text-[10px] px-1.5 py-0.5 rounded {model.capabilities?.[cap]
									? 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'
									: 'bg-gray-50 text-gray-400 dark:bg-gray-900'}"
								on:click={() =>
									patch(model, { capabilities: { ...model.capabilities, [cap]: !model.capabilities?.[cap] } })}
								>{$i18n.t(cap)}</button
							>
						{/each}
					</div>
					<button
						class="text-xs px-2 py-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 {defaultModel === model.id
							? 'text-amber-600'
							: 'text-gray-400'}"
						title={$i18n.t('Set as default')}
						on:click={() => setDefault(model)}>★</button
					>
					<button
						class="relative h-5 w-9 shrink-0 rounded-full transition {model.enabled
							? 'bg-emerald-600'
							: 'bg-gray-200 dark:bg-gray-700'}"
						aria-label={model.enabled ? $i18n.t('Enabled') : $i18n.t('Disabled')}
						on:click={() => patch(model, { enabled: !model.enabled })}
					>
						<span
							class="absolute top-0.5 left-0.5 size-4 rounded-full bg-white transition-transform {model.enabled
								? 'translate-x-4'
								: ''}"
						/>
					</button>
					<button class="text-xs text-red-500 px-1" on:click={() => remove(model)}>{$i18n.t('Delete')}</button>
				</div>
			{/each}
		</div>
	{/if}
</div>

<Modal size="sm" bind:show={showEdit}>
	{#if editing}
		<div class="p-5 space-y-3">
			<div class="text-lg font-medium">{$i18n.t('Edit Model')}</div>
			<div class="text-xs text-gray-500">{editing.provider_name} · {editing.model_id}</div>
			<div>
				<label class="block text-xs text-gray-500 mb-1" for="model-name">{$i18n.t('Display Name')}</label>
				<input
					id="model-name"
					class="w-full rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
					bind:value={form.name}
				/>
			</div>
			<div>
				<div class="text-xs text-gray-500 mb-1">{$i18n.t('Capabilities')}</div>
				<div class="flex flex-wrap gap-3 text-sm">
					{#each CAPS as cap}
						<label class="flex items-center gap-1">
							<input type="checkbox" bind:checked={form.capabilities[cap]} />
							{$i18n.t(cap)}
						</label>
					{/each}
				</div>
			</div>
			<div class="text-xs text-gray-500 pt-1">
				{$i18n.t('Default parameters (leave empty to use the provider default)')}
			</div>
			<div class="grid grid-cols-3 gap-2">
				<label class="text-xs text-gray-500"
					>Temperature
					<input
						class="w-full mt-1 rounded-lg px-2 py-1.5 text-sm bg-gray-50 dark:bg-gray-900"
						type="number"
						step="0.1"
						min="0"
						max="2"
						bind:value={form.temperature}
					/>
				</label>
				<label class="text-xs text-gray-500"
					>Top P
					<input
						class="w-full mt-1 rounded-lg px-2 py-1.5 text-sm bg-gray-50 dark:bg-gray-900"
						type="number"
						step="0.05"
						min="0"
						max="1"
						bind:value={form.top_p}
					/>
				</label>
				<label class="text-xs text-gray-500"
					>Max Tokens
					<input
						class="w-full mt-1 rounded-lg px-2 py-1.5 text-sm bg-gray-50 dark:bg-gray-900"
						type="number"
						min="1"
						bind:value={form.max_tokens}
					/>
				</label>
			</div>
			<label class="block text-xs text-gray-500"
				>{$i18n.t('Reasoning Effort')}
				<select
					class="w-full mt-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
					bind:value={form.reasoning_effort}
				>
					{#each EFFORTS as effort}
						<option value={effort}>{effort || $i18n.t('Default')}</option>
					{/each}
				</select>
			</label>
			<label class="block text-xs text-gray-500"
				>{$i18n.t('Input price')}
				<input class="w-full mt-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" type="number" min="0" step="0.01" bind:value={form.input_price} />
			</label>
			<label class="block text-xs text-gray-500"
				>{$i18n.t('Output price')}
				<input class="w-full mt-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900" type="number" min="0" step="0.01" bind:value={form.output_price} />
			</label>
			<label class="block text-xs text-gray-500"
				>{$i18n.t('System Prompt')}
				<textarea
					class="w-full mt-1 rounded-lg px-3 py-2 text-sm bg-gray-50 dark:bg-gray-900"
					rows="3"
					bind:value={form.system}
				/>
			</label>
			<div class="flex justify-end gap-2 pt-1">
				<button class="px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm" on:click={() => (showEdit = false)}
					>{$i18n.t('Cancel')}</button
				>
				<button
					class="px-3 py-1.5 rounded-lg bg-black text-white text-sm dark:bg-white dark:text-black"
					on:click={saveEdit}>{$i18n.t('Save')}</button
				>
			</div>
		</div>
	{/if}
</Modal>
