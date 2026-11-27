import { mount } from "svelte";
import "./shell/theme.css";
import { connect } from "./lib/transport";
import Shell from "./shell/Shell.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("#app element missing");

const transport = connect();
if (transport.remote) {
  // A remote machine's window has its own accent and title (ADR 0016).
  document.documentElement.dataset.remote = "";
  document.title = `Ovra on ${transport.remote.machine.name}`;
}
mount(Shell, { target, props: { transport } });
