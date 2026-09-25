import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import c from 'highlight.js/lib/languages/c';
import cpp from 'highlight.js/lib/languages/cpp';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import go from 'highlight.js/lib/languages/go';
import java from 'highlight.js/lib/languages/java';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import markdown from 'highlight.js/lib/languages/markdown';
import php from 'highlight.js/lib/languages/php';
import python from 'highlight.js/lib/languages/python';
import ruby from 'highlight.js/lib/languages/ruby';
import rust from 'highlight.js/lib/languages/rust';
import shell from 'highlight.js/lib/languages/shell';
import sql from 'highlight.js/lib/languages/sql';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

const builtin: Record<string, unknown> = {
	bash,
	c,
	cpp,
	css,
	diff,
	go,
	java,
	javascript,
	json,
	markdown,
	php,
	python,
	ruby,
	rust,
	shell,
	sql,
	typescript,
	xml,
	yaml
};

for (const [name, lang] of Object.entries(builtin)) {
	hljs.registerLanguage(name, lang as never);
}

const aliases: Record<string, string> = {
	js: 'javascript',
	ts: 'typescript',
	py: 'python',
	sh: 'bash',
	yml: 'yaml',
	html: 'xml',
	svg: 'xml',
	md: 'markdown',
	rs: 'rust'
};

const pending = new Map<string, Promise<void>>();

function escapeHtml(value: string) {
	return value
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;');
}

function loadLanguage(lang: string) {
	if (hljs.getLanguage(lang) || pending.has(lang)) return;
	const task = import(/* @vite-ignore */ `highlight.js/lib/languages/${lang}`)
		.then((mod) => {
			hljs.registerLanguage(lang, mod.default);
		})
		.catch(() => undefined);
	pending.set(lang, task);
}

export function highlightCode(code: string, lang: string) {
	const language = aliases[lang] || lang || 'plaintext';
	if (language === 'plaintext' || !hljs.getLanguage(language)) {
		if (language !== 'plaintext') loadLanguage(language);
		return escapeHtml(code);
	}
	return hljs.highlight(code, { language }).value;
}
