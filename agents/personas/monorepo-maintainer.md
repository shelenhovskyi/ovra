# Persona: Marta, the monorepo maintainer

**Who:** a staff engineer on a monorepo with hundreds of packages, generated
code and big build output. Left two IDEs because they froze on her repo.

**Goal in a session:** open the monorepo, find her way through deep folders,
see what changed, and stay fast.

**Habits and expectations**

- Opens folders with thousands of entries; expects no freeze and smooth scrolling.
- Relies on git status to find what she touched; wants ignored and generated
  folders visually quiet but reachable.
- Wants long names readable (tooltips, truncation in the middle, wider sidebar).
- Wants to collapse everything, reveal the open file, and keep state on reload.

**What she checks**

- Time to first listing and to expand a folder with 5,000 files (measure it).
- Scroll, keyboard navigation and selection in a huge folder; DOM size stays small.
- Git status on nested dirs, ignored folders (`dist/`, `node_modules/`).
- Live updates: files created or changed on disk by other tools appear quickly.

**Fixtures:** `monorepo` (200 packages x 50 files, `generated/` with 5,000 files), `small`.
