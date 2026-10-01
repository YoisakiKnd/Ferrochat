import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const baseline = Number(readFileSync(new URL('../.eslint-baseline', import.meta.url), 'utf8').trim());
if (!Number.isSafeInteger(baseline) || baseline < 0) {
	throw new Error('invalid ESLint baseline');
}
const result = spawnSync('./node_modules/.bin/eslint', ['.', '--format', 'json'], {
	encoding: 'utf8',
	maxBuffer: 16 * 1024 * 1024
});

if (result.error || ![0, 1].includes(result.status)) {
	process.stderr.write(result.stderr || String(result.error || `eslint exited ${result.status}`));
	process.exit(1);
}

let reports;
try {
	reports = JSON.parse(result.stdout);
} catch {
	process.stderr.write(result.stdout || result.stderr || 'eslint did not return JSON');
	process.exit(1);
}

const count = reports.reduce((total, report) => total + report.errorCount, 0);
console.log(`eslint errors: ${count} (baseline ${baseline})`);
if (count > baseline) {
	for (const report of reports.filter((report) => report.errorCount)) {
		console.error(`${report.filePath}: ${report.errorCount} errors`);
	}
	process.exit(1);
}
