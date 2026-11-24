import type { ProjectId } from "./projects";

export type OpenRequest = {
  method: "open.external";
  params: { project: ProjectId; path: string; line: number | null };
};
