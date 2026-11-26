import { mount } from "svelte";
import "../shell/theme.css";
import "./mobile.css";
import App from "./App.svelte";
import { browserStore } from "./identity";

const target = document.getElementById("app");
if (!target) throw new Error("#app element missing");

mount(App, { target, props: { store: browserStore, origin: location.origin } });
