import { loadHistory } from "../history/reader.ts";
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
			description: "Restore a file from backup (usage: /restore <file> [timestamp])",
			execute: async (args, _ctx) => {
				const parts = args.trim().split(/\s+/);
				const filePath = parts[0];
				if (!filePath) {
					console.log("  Usage: /restore <file-path> [timestamp]");
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
						console.log(`    ${b.timestamp} — ${b.size} bytes`);
					}
					console.log("  Use /restore <file> <timestamp> to restore");
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
	];
}
