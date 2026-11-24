import { mount } from "svelte";
import App from "./App.svelte";
import { connect } from "./lib/transport";

const target = document.getElementById("app");
if (!target) throw new Error("#app element missing");

mount(App, { target, props: { transport: connect() } });
