import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createBashTool(): HeddleTool {
	return {
		name: "bash",
		description: "Run a shell command and return its stdout and stderr.",
		parameters: Type.Object({
			command: Type.String({ description: "The shell command to execute" }),
		}),
		execute: async (params, options) => {
			const { command } = params as { command: string };
			const signal = options?.signal;

			if (signal?.aborted) {
				return "Error: Aborted";
			}

			try {
				const proc = Bun.spawn(["bash", "-c", command], {
					stdout: "pipe",
					stderr: "pipe",
				});

				if (signal) {
					const onAbort = () => {
						proc.kill();
					};
					signal.addEventListener("abort", onAbort, { once: true });
					// Clean up listener when process exits
					proc.exited.then(() => signal.removeEventListener("abort", onAbort));
				}

				const [stdout, stderr] = await Promise.all([
					new Response(proc.stdout).text(),
					new Response(proc.stderr).text(),
				]);
				const exitCode = await proc.exited;

				if (signal?.aborted) {
					return "Error: Aborted";
				}

				let output = "";
				if (stdout) output += stdout;
				if (stderr) output += `${output ? "\n" : ""}STDERR: ${stderr}`;
				if (exitCode !== 0) output += `${output ? "\n" : ""}Exit code: ${exitCode}`;
				return output || "(no output)";
			} catch (err) {
				if (signal?.aborted) {
					return "Error: Aborted";
				}
				const message = err instanceof Error ? err.message : String(err);
				return `Error: ${message}`;
			}
		},
	};
}
