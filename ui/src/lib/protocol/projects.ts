export type ProjectId = string;

export type Project = {
  id: ProjectId;
  name: string;
  root: string;
  last_opened_ms: number | null;
};

/** What a project looked like when it was last left. Paths are relative to the root. */
export type ProjectState = {
  /** Open tabs, in tab order. */
  open_files: string[];
  active_file: string | null;
  /** Expanded folders in the file tree. */
  expanded_dirs: string[];
};

export type ProjectsRequest =
  | { method: "projects.list" }
  | { method: "projects.add"; params: { root: string } }
  | { method: "projects.remove"; params: { id: ProjectId } }
  | { method: "projects.open"; params: { id: ProjectId } }
  | { method: "projects.get_state"; params: { id: ProjectId } }
  | { method: "projects.set_state"; params: { id: ProjectId; state: ProjectState } };

export type ProjectsResponse =
  | { type: "projects.list"; data: Project[] }
  | { type: "projects.project"; data: Project }
  | { type: "projects.state"; data: ProjectState };

export type ProjectsEvent = { type: "projects.changed"; data: { active: ProjectId | null } };
