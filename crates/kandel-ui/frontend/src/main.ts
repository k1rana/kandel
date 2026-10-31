import "./theme.css";
import { mount } from "svelte";

import App from "./App.svelte";

// The devtools dock, under the Vite dev server only. A shipped build never
// imports this, and `vite build` folds the guard to `false`. See `lib/devtools`.
if (import.meta.env.DEV) {
  void import("./lib/devtools").then((m) => m.installDevTools());
}

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
