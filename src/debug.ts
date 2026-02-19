const debugChannels = new Set<string>();
let debugAll = false;
let headless = false;

function initDebug(): void {
	debugChannels.clear();
	debugAll = false;
	const val = process.env.HEDDLE_DEBUG;
	if (!val) return;
	if (val === "1" || val === "true") {
		debugAll = true;
		return;
	}
	for (const ch of val.split(",")) debugChannels.add(ch.trim());
}
initDebug();

/** Re-read HEDDLE_DEBUG env var and reset state. Useful for testing. */
export function resetDebug(): void {
	initDebug();
}

export function setHeadless(value: boolean): void {
	headless = value;
}

export function debug(channel: string, ...args: unknown[]): void {
	if (!debugAll && !debugChannels.has(channel)) return;
	const prefix = `[heddle:${channel}]`;
	if (headless) {
		console.error(prefix, ...args);
	} else {
		console.debug(prefix, ...args);
	}
}
