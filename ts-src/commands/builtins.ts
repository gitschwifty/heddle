import { loadHistory } from "../history/reader.ts";
import { listPlans, loadPlan } from "../plans/storage.ts";
import { formatTasksSummary, loadTasks, saveTasks } from "../tasks/storage.ts";
import type { CommandRegistry } from "./registry.ts";
import type { SlashCommand } from "./types.ts";

export function createBuiltinCommands(commandRegistry: CommandRegistry): SlashCommand[] {
	return [
		{
			name: "help",
			description: "List available commands",
			execute: async () => {
				for (const cmd of commandRegistry.all()) {
					console.log(`  /${cmd.name} — ${cmd.description}`);
				}
			},
		},
		{
			name: "clear",
			description: "Clear conversation context",
			execute: async (_args, ctx) => {
				ctx.messages.length = 1;
				console.log("Context cleared.");
			},
		},
		{
			name: "exit",
			description: "Exit Heddle",
			execute: async (_args, ctx) => {
				console.log("Goodbye!");
				ctx.rl.close();
			},
		},
		{
			name: "quit",
			description: "Exit Heddle",
			execute: async (_args, ctx) => {
				console.log("Goodbye!");
				ctx.rl.close();
			},
		},
		{
			name: "cost",
			description: "Show token usage and cost",
			execute: async (_args, ctx) => {
				const { totalInputTokens, totalOutputTokens, totalCost } = ctx.costTracker;
				console.log(`  Input tokens:  ${totalInputTokens}`);
				console.log(`  Output tokens: ${totalOutputTokens}`);
				const costStr = totalCost !== null ? `$${totalCost.toFixed(4)}` : "N/A";
				console.log(`  Total cost:    ${costStr}`);
			},
		},
		{
			name: "status",
			description: "Show session status",
			execute: async (_args, ctx) => {
				console.log(`  Model:         ${ctx.config.model}`);
				console.log(`  Session:       ${ctx.sessionFile}`);
				console.log(`  Messages:      ${ctx.messages.length}`);
				console.log(`  Approval mode: ${ctx.config.approvalMode ?? "none"}`);
			},
		},
		{
			name: "context",
			description: "Show context size estimate",
			execute: async (_args, ctx) => {
				const totalChars = ctx.messages.reduce((sum, m) => {
					const content = typeof m.content === "string" ? m.content : "";
					return sum + content.length;
				}, 0);
				const estimatedTokens = Math.ceil(totalChars / 4);
				console.log(`  Messages:         ${ctx.messages.length}`);
				console.log(`  Estimated tokens: ~${estimatedTokens}`);
			},
		},
		{
			name: "model",
			description: "Switch model (e.g., /model openrouter/free)",
			execute: async (args, ctx) => {
				if (!args.trim()) {
					console.log(`  Current model: ${ctx.config.model}`);
					return;
				}
				ctx.setModel(args.trim());
				console.log(`  Model switched to: ${args.trim()}`);
			},
		},
		{
			name: "tools",
			description: "List available tools",
			execute: async (_args, ctx) => {
				for (const tool of ctx.registry.all()) {
					console.log(`  ${tool.name} — ${tool.description}`);
				}
			},
		},
		{
			name: "history",
			description: "Show recent message history",
			execute: async (args) => {
				const parts = args.trim().split(/\s+/).filter(Boolean);
				let limit = 20;
				let search: string | undefined;
				for (let i = 0; i < parts.length; i++) {
					const part = parts[i] as string;
					const next = parts[i + 1];
					if (part === "--limit" && next) {
						limit = Number.parseInt(next, 10) || 20;
						i++;
					} else if (part === "--search" && next) {
						search = parts.slice(i + 1).join(" ");
						break;
					} else if (part) {
						search = parts.slice(i).join(" ");
						break;
					}
				}
				const entries = await loadHistory({ limit, search });
				if (entries.length === 0) {
					console.log("  No history entries found.");
					return;
				}
				for (const entry of entries) {
					const time = entry.timestamp.replace("T", " ").replace(/\.\d+Z$/, "Z");
					console.log(`  [${time}] (${entry.content_type}) ${entry.message_preview}`);
				}
			},
		},
		{
			name: "restore",
			description: "Restore a file from backup (usage: /restore <file> [version])",
			execute: async (args, _ctx) => {
				const parts = args.trim().split(/\s+/);
				const filePath = parts[0];
				if (!filePath) {
					console.log("  Usage: /restore <file-path> [version]");
					return;
				}
				const { listBackups, restoreBackup } = await import("../file-history/restore.ts");
				if (parts[1]) {
					const result = await restoreBackup(filePath, Number(parts[1]));
					console.log(`  ${result}`);
				} else {
					const backups = await listBackups(filePath);
					if (backups.length === 0) {
						console.log(`  No backups found for ${filePath}`);
						return;
					}
					console.log(`  Backups for ${filePath}:`);
					for (const b of backups.slice(0, 10)) {
						console.log(`    v${b.version} — ${b.size} bytes`);
					}
					console.log("  Use /restore <file> <version> to restore");
				}
			},
		},
		{
			name: "compact",
			description: "Compact conversation context",
			execute: async (_args, ctx) => {
				if (!ctx.weakProvider) {
					console.log("  No weak model configured — cannot compact.");
					return;
				}
				const { compactContext } = await import("../context/compaction.ts");
				const modelLimit = ctx.config.maxTokens ?? 128000;
				const stats = await compactContext(ctx.messages, ctx.weakProvider, modelLimit);
				console.log(`  Compacted: removed ${stats.messagesRemoved} messages`);
				console.log(`  Tokens: ${stats.tokensBefore} → ${stats.tokensAfter}`);
			},
		},
		{
			name: "sessions",
			description: "List recent sessions",
			execute: async (_args, _ctx) => {
				const { listSessions } = await import("../session/list.ts");
				const sessions = await listSessions();
				if (sessions.length === 0) {
					console.log("  No sessions found.");
					return;
				}
				for (const s of sessions.slice(0, 20)) {
					const name = s.name ? ` (${s.name})` : "";
					const preview = s.firstUserMessage ? ` — ${s.firstUserMessage}` : "";
					console.log(`  ${s.id.slice(0, 8)}${name} | ${s.created} | ${s.messageCount} msgs${preview}`);
				}
			},
		},
		{
			name: "name",
			description: "Name the current session",
			execute: async (args, ctx) => {
				if (!args.trim()) {
					console.log("  Usage: /name <session-name>");
					return;
				}
				const { appendContextMarker } = await import("../session/jsonl.ts");
				await appendContextMarker(ctx.sessionFile, {
					type: "session_name",
					name: args.trim(),
					timestamp: new Date().toISOString(),
				});
				console.log(`  Session named: ${args.trim()}`);
			},
		},
		{
			name: "fork",
			description: "Fork the current session",
			execute: async (_args, ctx) => {
				const { forkSession } = await import("../session/fork.ts");
				const result = await forkSession(ctx.sessionFile);
				console.log(`  Forked to: ${result.sessionFile}`);
				console.log(`  New session ID: ${result.sessionId}`);
			},
		},
		{
			name: "tasks",
			description: "List or clear tracked tasks (usage: /tasks [clear])",
			execute: async (args, ctx) => {
				if (args.trim() === "clear") {
					const tasks = await loadTasks();
					const remaining = tasks.filter((t) => t.status !== "done");
					await saveTasks(remaining);
					console.log(`  Cleared ${tasks.length - remaining.length} completed tasks.`);
					return;
				}
				const tasks = await loadTasks();
				if (tasks.length === 0) {
					console.log("  No tasks tracked.");
					return;
				}
				console.log(formatTasksSummary(tasks, ctx.sessionId));
			},
		},
		{
			name: "agents",
			description: "List available agent definitions",
			execute: async (_args, ctx) => {
				if (ctx.agentDefinitions.size === 0) {
					console.log("  No agent definitions found.");
					return;
				}
				for (const [name, def] of ctx.agentDefinitions) {
					const model = def.model ? ` (${def.model})` : "";
					console.log(`  ${name}${model} — ${def.description}`);
				}
			},
		},
		{
			name: "plan",
			description: "Load or list saved plans (usage: /plan list | /plan load <name>)",
			execute: async (args) => {
				const parts = args.trim().split(/\s+/);
				const sub = parts[0];
				if (sub === "list" || !sub) {
					const plans = await listPlans();
					if (plans.length === 0) {
						console.log("  No saved plans.");
						return;
					}
					for (const p of plans) {
						const date = p.created ? p.created.replace("T", " ").slice(0, 19) : "unknown";
						console.log(`  ${p.name} (${date}) — ${p.preview}`);
					}
					return;
				}
				if (sub === "load") {
					const name = parts.slice(1).join(" ");
					if (!name) {
						console.log("  Usage: /plan load <name>");
						return;
					}
					const plan = await loadPlan(name);
					if (!plan) {
						console.log(`  Plan not found: "${name}"`);
						return;
					}
					console.log(`  Plan: ${plan.name}`);
					if (plan.meta.created) console.log(`  Created: ${plan.meta.created}`);
					if (plan.meta.model) console.log(`  Model: ${plan.meta.model}`);
					console.log(`\n${plan.content}`);
					return;
				}
				console.log("  Usage: /plan list | /plan load <name>");
			},
		},
		{
			name: "stats",
			description: "Show usage stats (usage: /stats [project])",
			execute: async (args, ctx) => {
				if (args.trim() === "project") {
					const { aggregateUsage } = await import("../usage/reader.ts");
					const { getProjectDir } = await import("../config/paths.ts");
					const stats = await aggregateUsage(getProjectDir());
					console.log(`  Sessions:      ${stats.totalSessions}`);
					console.log(`  Input tokens:  ${stats.totalTokens.input}`);
					console.log(`  Output tokens: ${stats.totalTokens.output}`);
					console.log(`  Total cost:    $${stats.totalCost.toFixed(4)}`);
					if (Object.keys(stats.toolCalls).length > 0) {
						console.log("  Tool calls:");
						for (const [tool, count] of Object.entries(stats.toolCalls).sort(([, a], [, b]) => b - a)) {
							console.log(`    ${tool}: ${count}`);
						}
					}
					return;
				}
				const { totalInputTokens, totalOutputTokens, totalCost } = ctx.costTracker;
				console.log(`  Input tokens:  ${totalInputTokens}`);
				console.log(`  Output tokens: ${totalOutputTokens}`);
				const costStr = totalCost !== null ? `$${totalCost.toFixed(4)}` : "N/A";
				console.log(`  Total cost:    ${costStr}`);
			},
		},
		{
			name: "paste",
			description: "Manage paste cache (usage: /paste [list|clear])",
			execute: async (args, ctx) => {
				if (!ctx.pasteCache) {
					console.log("  Paste cache is disabled.");
					return;
				}
				const sub = args.trim();
				if (sub === "clear") {
					ctx.pasteCache.clear();
					console.log("  Paste cache cleared.");
					return;
				}
				const entries = ctx.pasteCache.list();
				if (entries.length === 0) {
					console.log("  Paste cache is empty.");
					return;
				}
				for (const e of entries) {
					const id = e.pasteId ? ` [paste:${e.pasteId}]` : "";
					const size = Buffer.byteLength(e.content, "utf-8");
					console.log(`  ${e.path} (${e.lines} lines, ${size} bytes)${id}`);
				}
			},
		},
		{
			name: "agent",
			description: "Show agent definition details (usage: /agent <name>)",
			execute: async (args, ctx) => {
				const name = args.trim();
				if (!name) {
					console.log("  Usage: /agent <name>");
					console.log("  Use /agents to list available definitions.");
					return;
				}
				const def = ctx.agentDefinitions.get(name);
				if (!def) {
					console.log(`  Agent not found: "${name}"`);
					const available = [...ctx.agentDefinitions.keys()].join(", ");
					if (available) console.log(`  Available: ${available}`);
					return;
				}
				console.log(`  Name:        ${def.name}`);
				console.log(`  Description: ${def.description}`);
				if (def.model) console.log(`  Model:       ${def.model}`);
				if (def.tools) console.log(`  Tools:       ${def.tools.join(", ")}`);
				console.log(`  Source:      ${def.source}`);
				if (def.systemPrompt) {
					console.log(`  Prompt:      ${def.systemPrompt.slice(0, 200)}${def.systemPrompt.length > 200 ? "..." : ""}`);
				}
			},
		},
	];
}
