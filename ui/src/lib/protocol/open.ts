import type { ProjectId } from "./projects";

export type OpenRequest =
  | {
      method: "open.external";
      params: { project: ProjectId; path: string; line: number | null };
    }
  | {
      /** Opens a web page in the system browser; only http and https URLs. */
      method: "open.url";
      params: { url: string };
    };
