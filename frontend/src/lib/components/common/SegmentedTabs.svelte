<script lang="ts">
	import { createEventDispatcher } from 'svelte';

	export let value = '';
	export let options: { value: string; label: string }[] = [];

	const dispatch = createEventDispatcher();
</script>

<div class="flex gap-1">
	{#each options as option}
		<button
			type="button"
			class="px-3 py-1.5 rounded-lg text-sm {value === option.value
				? 'bg-gray-100 dark:bg-gray-800'
				: 'hover:bg-gray-50 dark:hover:bg-gray-850'}"
			on:click={() => {
				value = option.value;
				dispatch('change', value);
			}}
		>
			{option.label}
		</button>
	{/each}
</div>
