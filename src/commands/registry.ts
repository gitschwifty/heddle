import { findClosest } from "../tools/string-distance.ts";
import type { SlashCommand } from "./types.ts";

export class CommandRegistry {
	private commands = new Map<string, SlashCommand>();

	register(command: SlashCommand): void {
		this.commands.set(command.name, command);
	}

	get(name: string): SlashCommand | undefined {
		return this.commands.get(name);
	}

	all(): SlashCommand[] {
		return [...this.commands.values()];
	}

	suggest(name: string): string | undefined {
		const candidates = [...this.commands.keys()];
		return findClosest(name, candidates) ?? undefined;
	}
}
