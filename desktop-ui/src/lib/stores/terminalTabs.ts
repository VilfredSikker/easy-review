/** Last path segment of the drawer cwd, shown on the shell tab. */
export function terminalRepoName(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : "repo";
}
