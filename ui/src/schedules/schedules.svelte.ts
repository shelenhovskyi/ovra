// Scheduled tasks (ADR 0015): whether they are on, and each project's
// schedules, read once per transport and again on `schedules.changed`.
// The core keeps them and fires them; this only shows and edits them.

import type { ProjectId, Schedule, ScheduleDraft, ScheduleId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class Schedules {
  /** Whether schedules start agents; null until known or without a core that has them. */
  on = $state<boolean | null>(null);
  /** The schedules of the projects asked for, by project. */
  byProject = $state<Record<ProjectId, Schedule[]>>({});

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => {
      if (event.type !== "schedules.changed") return;
      if (event.data.project === null) void this.loadOn();
      // Switching on or off changes every next run.
      const projects = event.data.project === null ? Object.keys(this.byProject) : [event.data.project];
      for (const project of projects) if (project in this.byProject) void this.load(project);
    });
    // Deferred, so a fake that throws instead of rejecting leaves it unknown.
    void Promise.resolve().then(() => this.loadOn());
  }

  private async loadOn(): Promise<void> {
    try {
      this.on = (await this.transport.call({ method: "schedules.enabled" })).data.enabled;
    } catch {
      // A front end without schedules shows none.
    }
  }

  async setOn(enabled: boolean): Promise<void> {
    this.on = (await this.transport.call({ method: "schedules.set_enabled", params: { enabled } })).data.enabled;
  }

  list(project: ProjectId): Schedule[] {
    return this.byProject[project] ?? [];
  }

  async load(project: ProjectId): Promise<void> {
    const { data } = await this.transport.call({ method: "schedules.list", params: { project } });
    this.byProject = { ...this.byProject, [project]: data };
  }

  /** Makes a schedule, or with `id` changes it; answers it as the core kept it. */
  async save(project: ProjectId, draft: ScheduleDraft, id?: ScheduleId): Promise<Schedule> {
    const params = id ? { project, id, draft } : { project, draft };
    const { data } = await this.transport.call({ method: "schedules.save", params });
    this.put(data);
    return data;
  }

  async setPaused(id: ScheduleId, paused: boolean): Promise<void> {
    this.put((await this.transport.call({ method: "schedules.set_paused", params: { id, paused } })).data);
  }

  /** Starts a run now; answers the schedule with that run as its last. */
  async runNow(id: ScheduleId): Promise<Schedule> {
    const { data } = await this.transport.call({ method: "schedules.run_now", params: { id } });
    this.put(data);
    return data;
  }

  async remove(schedule: Schedule): Promise<void> {
    await this.transport.call({ method: "schedules.remove", params: { id: schedule.id } });
    const rest = this.list(schedule.project).filter((s) => s.id !== schedule.id);
    this.byProject = { ...this.byProject, [schedule.project]: rest };
  }

  private put(schedule: Schedule): void {
    const list = this.list(schedule.project);
    const at = list.findIndex((s) => s.id === schedule.id);
    const next = at < 0 ? [...list, schedule] : list.map((s, i) => (i === at ? schedule : s));
    this.byProject = { ...this.byProject, [schedule.project]: next };
  }
}

const instances = new WeakMap<Transport, Schedules>();

/** The schedules of `transport`, shared by Settings' switch and pane. */
export function schedules(transport: Transport): Schedules {
  let found = instances.get(transport);
  if (!found) {
    found = new Schedules(transport);
    instances.set(transport, found);
  }
  return found;
}
