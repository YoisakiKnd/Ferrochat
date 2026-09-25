<script lang="ts">
	import { onMount } from 'svelte';

	export let history;

	let Overview = null;
	let Provider = null;

	onMount(async () => {
		const [overview, flow] = await Promise.all([
			import('./Overview.svelte'),
			import('@xyflow/svelte')
		]);
		Overview = overview.default;
		Provider = flow.SvelteFlowProvider;
	});
</script>

{#if Overview && Provider}
	<svelte:component this={Provider}>
		<svelte:component this={Overview} {history} on:nodeclick on:close />
	</svelte:component>
{/if}
