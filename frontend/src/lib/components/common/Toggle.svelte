<script lang="ts">
	export let checked = false;
	export let label = '';
</script>

<label class="flex items-center gap-2 text-sm">
	<input class="size-4" type="checkbox" bind:checked on:change />
	{#if label}<span>{label}</span>{/if}
</label>
