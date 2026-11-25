export type ProjectId = string;

export type Project = {
  id: ProjectId;
  name: string;
  root: string;
  last_opened_ms: number | null;
  /** Whether the user trusts the project's own agent configuration; it loads only while unchanged. */
  trusted: boolean;
};

/** What a project looked like when it was last left. Paths are relative to the root. */
export type ProjectState = {
  /** Open tabs, in tab order. */
  open_files: string[];
  active_file: string | null;
  /** Expanded folders in the file tree. */
  expanded_dirs: string[];
  /**
   * Pinned tabs, a subset of `open_files`. Optional here so fixtures that
   * predate it stay valid; the core always sends it.
   */
  pinned_files?: string[];
  /** The editor group on the right, if the main area was split; absent with one group. */
  second_group?: EditorGroup;
};

/** The tabs of the second editor group, as `ProjectState` keeps the first's. */
export type EditorGroup = {
  open_files: string[];
  active_file: string | null;
  pinned_files: string[];
};

/**
 * Only the fields present are written, so panels save their own part without
 * overwriting each other's. `active_file: null` clears it; leaving it out keeps it.
 * `second_group` is written whole; one without open files closes the group.
 */
export type ProjectStatePatch = Partial<ProjectState>;

export type ProjectsRequest =
  | { method: "projects.list" }
  | { method: "projects.add"; params: { root: string } }
  | { method: "projects.remove"; params: { id: ProjectId } }
  | { method: "projects.open"; params: { id: ProjectId } }
  /**
   * Sessions started afterwards load the project's agent configuration or not.
   * Trusting needs the `fingerprint` of what the user reviewed; `conflict` if it changed.
   */
  | { method: "projects.trust"; params: { id: ProjectId; trusted: boolean; fingerprint?: string } }
  | { method: "projects.get_state"; params: { id: ProjectId } }
  | { method: "projects.set_state"; params: { id: ProjectId; state: ProjectStatePatch } };

export type ProjectsResponse =
  | { type: "projects.list"; data: Project[] }
  | { type: "projects.project"; data: Project }
  | { type: "projects.state"; data: ProjectState };

/**
 * The list or the set of open projects changed. `open` lists the projects
 * some client has open (each connection holds the one it opened last).
 */
export type ProjectsEvent = { type: "projects.changed"; data: { open: ProjectId[] } };
