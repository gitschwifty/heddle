import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import type { UsageRecord } from "./writer.ts";

export async function readUsageRecord(sessionId: string, projectDir: string): Promise<UsageRecord | null> {
	const filePath = join(projectDir, "usage", `${sessionId}.json`);
	try {
		const content = await readFile(filePath, "utf-8");
		return JSON.parse(content) as UsageRecord;
	} catch {
		return null;
	}
}

export async function aggregateUsage(projectDir: string): Promise<{
	totalSessions: number;
	totalTokens: { input: number; output: number };
	totalCost: number;
	toolCalls: Record<string, number>;
}> {
	const result = {
		totalSessions: 0,
		totalTokens: { input: 0, output: 0 },
		totalCost: 0,
		toolCalls: {} as Record<string, number>,
	};

	const usageDir = join(projectDir, "usage");
	let files: string[];
	try {
		files = await readdir(usageDir);
	} catch {
		return result;
	}

	for (const file of files) {
		if (!file.endsWith(".json")) continue;
		try {
			const content = await readFile(join(usageDir, file), "utf-8");
			const record = JSON.parse(content) as UsageRecord;
			result.totalSessions++;
			result.totalTokens.input += record.metrics.tokens.input;
			result.totalTokens.output += record.metrics.tokens.output;
			result.totalCost += record.cost_usd ?? 0;
			for (const [tool, count] of Object.entries(record.metrics.toolCalls)) {
				result.toolCalls[tool] = (result.toolCalls[tool] ?? 0) + count;
			}
		} catch {
			// Skip malformed files
		}
	}

	return result;
}
