<script lang="ts">
	import { v4 as uuidv4 } from 'uuid';

	import { getContext, onMount } from 'svelte';
	import { copyToClipboard } from '$lib/utils';

	import 'highlight.js/styles/github-dark.min.css';

	let SvgPanZoom = null;
	import ChevronUpDown from '$lib/components/icons/ChevronUpDown.svelte';

	let CodeEditor = null;

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	export let id = '';

	export let onSave = (e) => {};
	export let onCode = (e) => {};

	export let save = false;
	export let run = true;
	export let collapsed = false;

	export let token;
	export let lang = '';
	export let code = '';
	export let attributes = {};

	export let className = 'my-2';
	export let editorClassName = '';
	export let stickyButtonsClassName = 'top-8';

	let _code = '';
	$: if (code) {
		updateCode();
	}

	const updateCode = () => {
		_code = code;
	};

	let _token = null;

	let mermaidHtml = null;

	let highlightedCode = null;
	let executing = false;

	let stdout = null;
	let stderr = null;
	let result = null;
	let files = null;

	let copied = false;
	let saved = false;

	const collapseCodeBlock = () => {
		collapsed = !collapsed;
	};

	const saveCode = () => {
		saved = true;

		code = _code;
		onSave(code);

		setTimeout(() => {
			saved = false;
		}, 1000);
	};

	const copyCode = async () => {
		copied = true;
		await copyToClipboard(code);

		setTimeout(() => {
			copied = false;
		}, 1000);
	};

	let debounceTimeout;

	const drawMermaidDiagram = async () => {
		try {
			const mermaid = (await import('mermaid')).default;
			mermaid.initialize({
				startOnLoad: false,
				theme: document.documentElement.classList.contains('dark') ? 'dark' : 'default',
				securityLevel: 'loose'
			});
			if (await mermaid.parse(code)) {
				const { svg } = await mermaid.render(`mermaid-${uuidv4()}`, code);
				mermaidHtml = svg;
			}
		} catch (error) {
			console.log('Error:', error);
		}
	};

	const render = async () => {
		if (lang === 'mermaid' && (token?.raw ?? '').slice(-4).includes('```')) {
			(async () => {
				await drawMermaidDiagram();
			})();
		}
	};

	$: if (token) {
		if (JSON.stringify(token) !== JSON.stringify(_token)) {
			_token = token;
		}
	}

	$: if (_token) {
		render();
	}

	$: onCode({ lang, code });

	$: if (attributes) {
		onAttributesUpdate();
	}

	const onAttributesUpdate = () => {
		if (attributes?.output) {
			// Create a helper function to unescape HTML entities
			const unescapeHtml = (html) => {
				const textArea = document.createElement('textarea');
				textArea.innerHTML = html;
				return textArea.value;
			};

			try {
				// Unescape the HTML-encoded string
				const unescapedOutput = unescapeHtml(attributes.output);

				// Parse the unescaped string into JSON
				const output = JSON.parse(unescapedOutput);

				// Assign the parsed values to variables
				stdout = output.stdout;
				stderr = output.stderr;
				result = output.result;
			} catch (error) {
				console.error('Error:', error);
			}
		}
	};

	onMount(async () => {
		if (lang) {
			onCode({ lang, code });
		}
		if (lang === 'mermaid') {
			SvgPanZoom = (await import('$lib/components/common/SVGPanZoom.svelte')).default;
		} else {
			CodeEditor = (await import('$lib/components/common/CodeEditor.svelte')).default;
		}
	});
</script>

<div>
	<div class="relative {className} flex flex-col rounded-lg" dir="ltr">
		{#if lang === 'mermaid'}
			{#if mermaidHtml && SvgPanZoom}
				<svelte:component this={SvgPanZoom}
					className=" border border-gray-100 dark:border-gray-850 rounded-lg max-h-fit overflow-hidden"
					svg={mermaidHtml}
					content={_token.text}
				/>
			{:else}
				<pre class="mermaid">{code}</pre>
			{/if}
		{:else}
			<div class="text-text-300 absolute pl-4 py-1.5 text-xs font-medium dark:text-white">
				{lang}
			</div>

			<div
				class="sticky {stickyButtonsClassName} mb-1 py-1 pr-2.5 flex items-center justify-end z-10 text-xs text-black dark:text-white"
			>
				<div class="flex items-center gap-0.5 translate-y-[1px]">
					<button
						class="flex gap-1 items-center bg-none border-none bg-gray-50 hover:bg-gray-100 dark:bg-gray-850 dark:hover:bg-gray-800 transition rounded-md px-1.5 py-0.5"
						on:click={collapseCodeBlock}
					>
						<div>
							<ChevronUpDown className="size-3" />
						</div>

						<div>
							{collapsed ? $i18n.t('Expand') : $i18n.t('Collapse')}
						</div>
					</button>

					{#if save}
						<button
							class="save-code-button bg-none border-none bg-gray-50 hover:bg-gray-100 dark:bg-gray-850 dark:hover:bg-gray-800 transition rounded-md px-1.5 py-0.5"
							on:click={saveCode}
						>
							{saved ? $i18n.t('Saved') : $i18n.t('Save')}
						</button>
					{/if}

					<button
						class="copy-code-button bg-none border-none bg-gray-50 hover:bg-gray-100 dark:bg-gray-850 dark:hover:bg-gray-800 transition rounded-md px-1.5 py-0.5"
						on:click={copyCode}>{copied ? $i18n.t('Copied') : $i18n.t('Copy')}</button
					>
				</div>
			</div>

			<div
				class="language-{lang} rounded-t-lg -mt-8 {editorClassName
					? editorClassName
					: executing || stdout || stderr || result
						? ''
						: 'rounded-b-lg'} overflow-hidden"
			>
				<div class=" pt-7 bg-gray-50 dark:bg-gray-850"></div>

				{#if !collapsed}
					{#if CodeEditor}
						<svelte:component
							this={CodeEditor}
							value={code}
							{id}
							{lang}
							onSave={() => {
								saveCode();
							}}
							onChange={(value) => {
								_code = value;
							}}
						/>
					{:else}
						<pre class="px-4 py-3 text-sm overflow-x-auto">{code}</pre>
					{/if}
				{:else}
					<div
						class="bg-gray-50 dark:bg-black dark:text-white rounded-b-lg! pt-2 pb-2 px-4 flex flex-col gap-2 text-xs"
					>
						<span class="text-gray-500 italic">
							{$i18n.t('{{COUNT}} hidden lines', {
								COUNT: code.split('\n').length
							})}
						</span>
					</div>
				{/if}
			</div>

			{#if !collapsed}
				<div
					id="plt-canvas-{id}"
					class="bg-gray-50 dark:bg-[#202123] dark:text-white max-w-full overflow-x-auto scrollbar-hidden"
				/>

				{#if executing || stdout || stderr || result || files}
					<div
						class="bg-gray-50 dark:bg-[#202123] dark:text-white rounded-b-lg! py-4 px-4 flex flex-col gap-2"
					>
						{#if executing}
							<div class=" ">
								<div class=" text-gray-500 text-xs mb-1">STDOUT/STDERR</div>
								<div class="text-sm">Running...</div>
							</div>
						{:else}
							{#if stdout || stderr}
								<div class=" ">
									<div class=" text-gray-500 text-xs mb-1">STDOUT/STDERR</div>
									<div
										class="text-sm {stdout?.split('\n')?.length > 100
											? `max-h-96`
											: ''}  overflow-y-auto"
									>
										{stdout || stderr}
									</div>
								</div>
							{/if}
							{#if result || files}
								<div class=" ">
									<div class=" text-gray-500 text-xs mb-1">RESULT</div>
									{#if result}
										<div class="text-sm">{`${JSON.stringify(result)}`}</div>
									{/if}
									{#if files}
										<div class="flex flex-col gap-2">
											{#each files as file}
												{#if file.type.startsWith('image')}
													<img src={file.data} alt="Output" class=" w-full max-w-[36rem]" />
												{/if}
											{/each}
										</div>
									{/if}
								</div>
							{/if}
						{/if}
					</div>
				{/if}
			{/if}
		{/if}
	</div>
</div>
