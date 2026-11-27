#!/usr/bin/env python3
"""Fail if workspace crates depend on each other outside scripts/crate-graph.json,
or pull in a banned crate (normal dependencies, transitively, with the
crate's own features)."""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
rules = json.loads((ROOT / "scripts" / "crate-graph.json").read_text())
meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT
    )
)

pkg_name = {p["id"]: p["name"] for p in meta["packages"]}
members = {pkg_name[m] for m in meta["workspace_members"]}
graph = {}
for node in meta["resolve"]["nodes"]:
    graph[node["id"]] = [
        d["pkg"]
        for d in node["deps"]
        if any(k["kind"] is None for k in d["dep_kinds"])
    ]
by_name = {pkg_name[i]: i for i in meta["workspace_members"]}

errors = []
for name in sorted(members):
    if name not in rules["allowed"]:
        errors.append(f"{name}: not listed in scripts/crate-graph.json")
        continue
    direct = {pkg_name[d] for d in graph[by_name[name]]} & members
    for dep in sorted(direct - set(rules["allowed"][name])):
        errors.append(f"{name} -> {dep}: edge not allowed")

    seen, stack = set(), [by_name[name]]
    while stack:
        for dep in graph[stack.pop()]:
            if dep not in seen:
                seen.add(dep)
                stack.append(dep)
    reachable = {pkg_name[d] for d in seen}
    hits = [b for b in rules["banned"].get(name, []) if b in reachable]
    if hits:
        # `cargo metadata` unifies features across the workspace, so a
        # crate seems to reach what only another member's features turn
        # on. `cargo tree -p` resolves this crate's own features.
        tree = subprocess.check_output(
            ["cargo", "tree", "-q", "--locked", "-p", name, "-e", "normal",
             "--target", "all", "--prefix", "none"],
            cwd=ROOT,
            text=True,
        )
        own = {line.split()[0] for line in tree.splitlines() if line.split()}
        for banned in hits:
            if banned in own:
                errors.append(f"{name}: depends on banned crate {banned}")

for e in errors:
    print(f"crate-graph: {e}", file=sys.stderr)
sys.exit(1 if errors else 0)
