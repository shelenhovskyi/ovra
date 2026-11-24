export type ProjectId = string;

export type Project = {
  id: ProjectId;
  name: string;
  root: string;
  last_opened_ms: number | null;
};

export type ProjectsRequest =
  | { method: "projects.list" }
  | { method: "projects.add"; params: { root: string } }
  | { method: "projects.remove"; params: { id: ProjectId } }
  | { method: "projects.open"; params: { id: ProjectId } };

export type ProjectsResponse = { type: "projects.list"; data: Project[] } | { type: "projects.project"; data: Project };

export type ProjectsEvent = { type: "projects.changed"; data: { active: ProjectId | null } };
