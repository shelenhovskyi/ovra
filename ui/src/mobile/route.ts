// Screens are addressed by the URL's hash, so Back on the phone works:
// `#/` Home, `#/approval/<id>`, `#/agent/<session>`, `#/task/<id>`, `#/computer`.

export type Route =
  | { screen: "home" }
  | { screen: "approval"; id: string }
  | { screen: "agent"; session: string }
  | { screen: "task"; id: string }
  | { screen: "computer" };

export function parseRoute(hash: string): Route {
  const [, screen, id] = /^#\/([a-z]+)(?:\/(.+))?$/.exec(hash) ?? [];
  const arg = id ? decodeURIComponent(id) : "";
  if (screen === "approval" && arg) return { screen, id: arg };
  if (screen === "agent" && arg) return { screen, session: arg };
  if (screen === "task" && arg) return { screen, id: arg };
  if (screen === "computer") return { screen };
  return { screen: "home" };
}

export function href(route: Route): string {
  switch (route.screen) {
    case "home":
      return "#/";
    case "approval":
      return `#/approval/${encodeURIComponent(route.id)}`;
    case "agent":
      return `#/agent/${encodeURIComponent(route.session)}`;
    case "task":
      return `#/task/${encodeURIComponent(route.id)}`;
    case "computer":
      return "#/computer";
  }
}
