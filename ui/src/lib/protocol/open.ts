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
    }
  | {
      /** Shows a file or folder in the system file manager; `""` is the project folder. */
      method: "open.reveal";
      params: { project: ProjectId; path: string };
    };
