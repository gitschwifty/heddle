export interface ShellResult {
	stdout: string;
	stderr: string;
	exitCode: number;
}

export async function runShell(command: string): Promise<ShellResult> {
	const proc = Bun.spawn(["bash", "-c", command], {
		stdout: "pipe",
		stderr: "pipe",
	});
	const [stdout, stderr] = await Promise.all([new Response(proc.stdout).text(), new Response(proc.stderr).text()]);
	const exitCode = await proc.exited;
	return { stdout, stderr, exitCode };
}

export function printShellResult(result: ShellResult): void {
	if (result.stdout) {
		process.stdout.write(result.stdout);
	}
	if (result.stderr) {
		process.stderr.write(result.stderr);
	}
	if (result.exitCode !== 0) {
		process.stderr.write(`Exit code: ${result.exitCode}\n`);
	}
}

export function formatShellForContext(command: string, result: ShellResult): { role: "user"; content: string } {
	let content = `Shell output from \`${command}\`:\n\`\`\`\n${result.stdout}\`\`\``;
	if (result.stderr) {
		content += `\nstderr:\n\`\`\`\n${result.stderr}\`\`\``;
	}
	return { role: "user", content };
}
