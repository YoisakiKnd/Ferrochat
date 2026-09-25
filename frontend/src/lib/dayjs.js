import dayjs from 'dayjs';

import 'dayjs/locale/en';
import 'dayjs/locale/zh-cn';
import 'dayjs/locale/zh-tw';

const loaded = new Set(['en', 'zh-cn', 'zh-tw']);

export async function loadDayjsLocale(locales = []) {
	for (const locale of locales) {
		const code = String(locale).toLowerCase();
		if (!loaded.has(code)) {
			try {
				await import(`dayjs/locale/${code}.js`);
				loaded.add(code);
			} catch (error) {
				continue;
			}
		}
		dayjs.locale(code);
		return code;
	}
	dayjs.locale('en');
	return 'en';
}

export default dayjs;
