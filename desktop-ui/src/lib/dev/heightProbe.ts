/** Whether the dev row-height validator is on (`localStorage.erDevHeightProbe = "1"`).
 *  Read here so the diff view, which imports wire types, touches no storage. */
export function devHeightProbeOn(): boolean {
  return globalThis.localStorage?.getItem("erDevHeightProbe") === "1";
}
