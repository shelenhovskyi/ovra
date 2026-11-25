import { mount } from "svelte";
import "./shell/theme.css";
import { connect } from "./lib/transport";
import Shell from "./shell/Shell.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("#app element missing");

mount(Shell, { target, props: { transport: connect() } });
