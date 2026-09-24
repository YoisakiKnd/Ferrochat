<script>
	import { getContext, onMount } from 'svelte';
	import { page } from '$app/stores';
	import General from './Settings/General.svelte';
	import Interface from './Settings/Interface.svelte';
	import Providers from './Settings/Providers.svelte';
	import Models from './Settings/Models.svelte';

	const i18n = getContext('i18n');
	const tabs = [
		['providers', 'Providers'],
		['models', 'Models'],
		['general', 'General'],
		['interface', 'Interface']
	];
	let selectedTab = tabs.some(([id]) => id === $page.url.searchParams.get('tab'))
		? $page.url.searchParams.get('tab')
		: 'providers';

	const go = (tab) => {
		selectedTab = tab;
		const url = new URL(window.location.href);
		url.searchParams.set('tab', tab);
		history.replaceState(history.state, '', url);
	};

	onMount(() => {
		const containerElement = document.getElementById('admin-settings-tabs-container');
		if (containerElement) {
			containerElement.addEventListener('wheel', function (event) {
				if (event.deltaY !== 0) {
					containerElement.scrollLeft += event.deltaY;
				}
			});
		}
	});
</script>

<div class="flex flex-col lg:flex-row w-full h-full pb-2 lg:space-x-4">
	<div
		id="admin-settings-tabs-container"
		class="tabs flex flex-row overflow-x-auto gap-2.5 max-w-full lg:gap-1 lg:flex-col lg:flex-none lg:w-40 dark:text-gray-200 text-sm font-medium text-left scrollbar-none"
	>
		{#each tabs as [id, label]}
			<button
				class="px-2 py-1.5 min-w-fit rounded-lg text-left transition {selectedTab === id
					? 'bg-gray-100 dark:bg-gray-800'
					: 'text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-white'}"
				on:click={() => go(id)}>{$i18n.t(label)}</button
			>
		{/each}
	</div>

	<div class="flex-1 mt-3 lg:mt-0 overflow-y-scroll">
		{#if selectedTab === 'providers'}
			<Providers on:models={() => go('models')} />
		{:else if selectedTab === 'models'}
			<Models on:providers={() => go('providers')} />
		{:else if selectedTab === 'general'}
			<General saveHandler={() => {}} on:save={() => {}} />
		{:else if selectedTab === 'interface'}
			<Interface />
		{/if}
	</div>
</div>
