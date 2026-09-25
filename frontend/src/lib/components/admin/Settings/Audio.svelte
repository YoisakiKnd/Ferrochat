<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { WEBUI_API_BASE_URL } from '$lib/constants';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	let sttEngine = 'web';
	let ttsEngine = '';
	let url = '';
	let apiKey = '';
	let sttModel = 'whisper-1';
	let ttsModel = 'tts-1';
	let voice = 'alloy';

	async function load() {
		const res = await fetch(`${WEBUI_API_BASE_URL}/audio/config`, {
			headers: { Authorization: `Bearer ${localStorage.token}` }
		});
		if (!res.ok) return;
		const data = await res.json();
		sttEngine = data.stt_engine || 'web';
		ttsEngine = data.tts_engine || '';
		url = data.url || '';
		apiKey = data.api_key || '';
		sttModel = data.stt_model || 'whisper-1';
		ttsModel = data.tts_model || 'tts-1';
		voice = data.voice || 'alloy';
	}

	async function save() {
		const res = await fetch(`${WEBUI_API_BASE_URL}/audio/config`, {
			method: 'POST',
			headers: {
				Authorization: `Bearer ${localStorage.token}`,
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({
				stt_engine: sttEngine,
				tts_engine: ttsEngine,
				url,
				api_key: apiKey,
				stt_model: sttModel,
				tts_model: ttsModel,
				voice
			})
		});
		if (!res.ok) {
			toast.error($i18n.t('Failed to save'));
			return;
		}
		toast.success($i18n.t('Saved'));
	}

	onMount(load);
</script>

<div class="flex flex-col gap-3 text-sm max-w-xl">
	<div class="text-sm font-medium">{$i18n.t('Audio')}</div>
	<div class="text-xs text-gray-500">
		{$i18n.t('Voice input uses the browser by default. An OpenAI-compatible endpoint is optional for transcription and speech.')}
	</div>
	<label class="flex flex-col gap-1">
		<span>{$i18n.t('Speech to text')}</span>
		<select class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={sttEngine}>
			<option value="web">{$i18n.t('Browser')}</option>
			<option value="openai">{$i18n.t('OpenAI-compatible')}</option>
		</select>
	</label>
	<label class="flex flex-col gap-1">
		<span>{$i18n.t('Text to speech')}</span>
		<select class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={ttsEngine}>
			<option value="">{$i18n.t('Browser')}</option>
			<option value="openai">{$i18n.t('OpenAI-compatible')}</option>
		</select>
	</label>
	{#if sttEngine === 'openai' || ttsEngine === 'openai'}
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Base URL')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={url} placeholder="https://api.openai.com/v1" />
		</label>
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('API Key')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" type="password" bind:value={apiKey} />
		</label>
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Transcription model')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={sttModel} />
		</label>
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Speech model')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={ttsModel} />
		</label>
		<label class="flex flex-col gap-1">
			<span>{$i18n.t('Voice')}</span>
			<input class="rounded-lg bg-gray-50 dark:bg-gray-850 px-3 py-2" bind:value={voice} />
		</label>
	{/if}
	<button type="button" class="w-fit px-3 py-1.5 rounded-lg bg-gray-900 text-white dark:bg-white dark:text-black" on:click={save}>
		{$i18n.t('Save')}
	</button>
</div>
