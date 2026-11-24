// Short, distinct labels for tabs.

export type TabLabel = { name: string; hint: string };

/**
 * The file name of each path, and for names that several paths share the
 * shortest trailing folder part that tells them apart, like VS Code.
 */
export function tabLabels(paths: readonly string[]): Map<string, TabLabel> {
  const byName = new Map<string, string[]>();
  for (const path of paths) {
    const name = path.slice(path.lastIndexOf("/") + 1);
    byName.set(name, [...(byName.get(name) ?? []), path]);
  }
  const labels = new Map<string, TabLabel>();
  for (const [name, group] of byName) {
    const dirs = group.map((path) => path.split("/").slice(0, -1));
    group.forEach((path, i) => {
      const own = dirs[i] ?? [];
      if (group.length === 1) {
        labels.set(path, { name, hint: "" });
        return;
      }
      // Fewest trailing folders that no other path in the group ends with.
      let depth = 1;
      const suffix = (parts: string[], n: number) => parts.slice(Math.max(0, parts.length - n)).join("/");
      while (depth < own.length && dirs.some((other, j) => j !== i && suffix(other, depth) === suffix(own, depth))) {
        depth++;
      }
      // Where the differing part is further up, show it with an ellipsis for the rest.
      const hint =
        own.length === 0 ? "./" : depth > 1 && depth < own.length ? `.../${suffix(own, depth)}` : suffix(own, depth);
      labels.set(path, { name, hint });
    });
  }
  return labels;
}
