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
	];
}
