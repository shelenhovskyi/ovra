import { defineConfig } from "astro/config";

// Served by GitHub Pages at https://shelenhovskyi.github.io/ovra/.
export default defineConfig({
  site: "https://shelenhovskyi.github.io",
  base: "/ovra",
  trailingSlash: "always",
  markdown: {
    shikiConfig: { themes: { light: "github-light", dark: "github-dark" } },
  },
});
