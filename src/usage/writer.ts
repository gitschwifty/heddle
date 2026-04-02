import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { SessionMetrics } from "./collector.ts";

export interface UsageRecord {
	session_id: string;
	project: string;
	created: string;
	ended: string;
	duration_ms: number;
	metrics: SessionMetrics;
	cost_usd?: number;
}

export async function writeUsageRecord(record: UsageRecord, projectDir: string): Promise<void> {
	const usageDir = join(projectDir, "usage");
	await mkdir(usageDir, { recursive: true });
	const filePath = join(usageDir, `${record.session_id}.json`);
	await writeFile(filePath, JSON.stringify(record, null, 2), "utf-8");
}
