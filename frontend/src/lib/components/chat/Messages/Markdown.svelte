<script>
	import { replaceTokens, processResponseContent } from '$lib/utils';
	import { ensureMarked } from '$lib/utils/marked';
	import { user } from '$lib/stores';

	import markedExtension from '$lib/utils/marked/extension';
	import markedKatexExtension from '$lib/utils/marked/katex-extension';

	import MarkdownTokens from './Markdown/MarkdownTokens.svelte';
	import { createEventDispatcher } from 'svelte';

	const dispatch = createEventDispatcher();

	export let id = '';
	export let content;
	export let model = null;
	export let save = false;

	export let sourceIds = [];

	export let onSourceClick = () => {};
	export let onTaskClick = () => {};

	let tokens = [];
	let parsedContent = '';
	let frame = 0;
	let cachedPrefix = '';
	let cachedTokens = [];

	const options = {
		throwOnError: false
	};

	async function renderContent(value) {
		if (!value) {
			tokens = [];
			parsedContent = value;
			return;
		}
		if (value === parsedContent && tokens.length) return;
		const marked = await ensureMarked((marked) => {
			marked.use(markedKatexExtension(options));
			marked.use(markedExtension(options));
		});
		const lex = (text) =>
			marked.lexer(replaceTokens(processResponseContent(text), sourceIds, model?.name, $user?.name));
		const splitAt = value.lastIndexOf('\n\n');
		if (value !== content) return;
		if (splitAt > 32 && value.slice(0, splitAt) === cachedPrefix && cachedTokens.length) {
			tokens = [...cachedTokens, ...lex(value.slice(splitAt))];
		} else {
			tokens = lex(value);
			if (splitAt > 32) {
				cachedPrefix = value.slice(0, splitAt);
				cachedTokens = lex(cachedPrefix);
			}
		}
		parsedContent = value;
	}

	$: {
		const value = content;
		cancelAnimationFrame(frame);
		frame = requestAnimationFrame(() => renderContent(value));
	}
</script>

{#key id}
	<MarkdownTokens
		{tokens}
		{id}
		{save}
		{onTaskClick}
		{onSourceClick}
		on:update={(e) => {
			dispatch('update', e.detail);
		}}
		on:code={(e) => {
			dispatch('code', e.detail);
		}}
	/>
{/key}
