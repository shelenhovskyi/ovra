// File and folder icons from Material Icon Theme (MIT, see NOTICE), muted for
// the Quiet style. The table (about 40 KB gzipped) loads on first use, so the
// tree's first paint does not wait for it: rows show an empty slot until then.

type Data = typeof import("./data")["default"];

let data = $state<Data | null>(null);
let loading: Promise<void> | null = null;
let light = $state(false);

/** Starts loading the table once; rows render plain until it arrives. */
export function loadIcons(): Promise<void> {
  if (!loading) {
    const query = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;
    if (query) {
      light = query.matches;
      query.addEventListener("change", (e) => {
        light = e.matches;
      });
    }
    loading = import("./data").then((m) => {
      data = m.default;
    });
  }
  return loading;
}

/** `_api`, `__api__` and `.api` find the icon of `api` (scripts/file_icons.py). */
function short(name: string): string {
  return name.replace(/^[._]+|[._]+$/g, "") || name;
}

function lookup(table: Record<string, string>, key: string): string | undefined {
  return Object.hasOwn(table, key) ? table[key] : undefined;
}

/** The theme's icon name for a row: exact name first, then the longest known extension. */
export function iconName(icons: Data, name: string, kind: "file" | "dir" | "symlink", open: boolean): string {
  const lower = name.toLowerCase();
  if (kind === "dir") {
    const folder =
      lookup(icons.folderNames, name) ??
      lookup(icons.folderNames, lower) ??
      lookup(icons.folderNames, short(lower)) ??
      "folder";
    return open ? `${folder}-open` : folder;
  }
  const named = lookup(icons.fileNames, name) ?? lookup(icons.fileNames, lower);
  if (named) return named;
  for (let dot = lower.indexOf(".", 1); dot !== -1; dot = lower.indexOf(".", dot + 1)) {
    const icon = lookup(icons.extensions, lower.slice(dot + 1));
    if (icon) return icon;
  }
  return "file";
}

const urls = new Map<string, string>();

/** A data URL for the row's icon (the CSP allows `data:` images), or null while the table loads. */
export function iconUrl(name: string, kind: "file" | "dir" | "symlink", open: boolean): string | null {
  const icons = data;
  if (!icons) return null;
  const icon = iconName(icons, name, kind, open);
  const variant = light && Object.hasOwn(icons.light, icon) ? "light" : "svg";
  const key = `${variant}:${icon}`;
  let url = urls.get(key);
  if (!url) {
    const table = icons[variant];
    url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(table[icon] ?? icons.svg.file ?? "")}`;
    urls.set(key, url);
  }
  return url;
}
