type Marked = typeof import('marked').marked;

let instance: Marked | null = null;
let loading: Promise<Marked> | null = null;

export const getMarked = () => instance;

let configured = false;

export const ensureMarked = (configure?: (marked: Marked) => void) => {
	const ready = instance
		? Promise.resolve(instance)
		: (loading ??= import('marked').then(({ marked }) => {
				instance = marked;
				return marked;
			}));
	return ready.then((marked) => {
		if (configure && !configured) {
			configure(marked);
			configured = true;
		}
		return marked;
	});
};
