<script lang="ts">
	export let value = '';
	export let options: { value: string; label: string }[] = [];
	export let className = '';
</script>

<select
	class="rounded-lg bg-transparent border border-gray-200 dark:border-gray-800 px-2 py-1.5 text-sm {className}"
	bind:value
	on:change
>
	{#each options as option}
		<option value={option.value}>{option.label}</option>
	{/each}
</select>
