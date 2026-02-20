import { appendFileSync, writeFileSync } from "node:fs";

const debugChannels = new Set<string>();
let debugAll = false;
let headless = false;
let logFile: string | null = null;

function initDebug(): void {
	debugChannels.clear();
	debugAll = false;
	logFile = process.env.HEDDLE_DEBUG_FILE ?? null;
	const val = process.env.HEDDLE_DEBUG;
	if (!val) return;
	if (val === "1" || val === "true") {
		debugAll = true;
		return;
	}
	for (const ch of val.split(",")) debugChannels.add(ch.trim());
}
initDebug();

/** Re-read HEDDLE_DEBUG and HEDDLE_DEBUG_FILE env vars and reset state. Useful for testing. */
export function resetDebug(): void {
	initDebug();
}

export function setHeadless(value: boolean): void {
	headless = value;
}

function formatArgs(args: unknown[]): string {
	return args
		.map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
		.join(" ");
}

export function debug(channel: string, ...args: unknown[]): void {
	if (!debugAll && !debugChannels.has(channel)) return;
	const prefix = `[heddle:${channel}]`;
	if (logFile) {
		const timestamp = new Date().toISOString();
		appendFileSync(logFile, `${timestamp} ${prefix} ${formatArgs(args)}\n`);
	} else if (headless) {
		console.error(prefix, ...args);
	} else {
		console.debug(prefix, ...args);
	}
}

/** Clear the log file if one is configured. No-op otherwise. */
export function clearLogFile(): void {
	if (logFile) writeFileSync(logFile, "");
}
