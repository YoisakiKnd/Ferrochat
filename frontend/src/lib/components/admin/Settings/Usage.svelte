<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { WEBUI_API_BASE_URL } from '$lib/constants';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');
	let rows: {
		model: string;
		day: string;
		prompt_tokens: number;
		completion_tokens: number;
		cost: number;
	}[] = [];

	onMount(async () => {
		const res = await fetch(`${WEBUI_API_BASE_URL}/usage`, {
			headers: { Authorization: `Bearer ${localStorage.token}` }
		});
		if (res.ok) rows = await res.json();
	});
</script>

<div class="text-sm max-w-3xl">
	<div class="font-medium mb-2">{$i18n.t('Usage')}</div>
	<div class="text-xs text-gray-500 mb-3">
		{$i18n.t('Token counts are estimated. Cost uses input_price and output_price per million tokens on the model.')}
	</div>
	<table class="w-full text-left text-xs">
		<thead>
			<tr class="text-gray-500">
				<th class="py-1">{$i18n.t('Day')}</th>
				<th>{$i18n.t('Model')}</th>
				<th>{$i18n.t('Prompt')}</th>
				<th>{$i18n.t('Completion')}</th>
				<th>{$i18n.t('Cost')}</th>
			</tr>
		</thead>
		<tbody>
			{#each rows as row}
				<tr>
					<td class="py-1">{row.day}</td>
					<td>{row.model}</td>
					<td>{row.prompt_tokens}</td>
					<td>{row.completion_tokens}</td>
					<td>{row.cost.toFixed(4)}</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
