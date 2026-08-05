// @ts-check

import starlight from "@astrojs/starlight"
import { defineConfig } from "astro/config"

// https://astro.build/config
export default defineConfig({
  site: "https://akhansari.gitlab.io",
  base: "/dbcrab",
  publicDir: "../docs/assets",
  integrations: [
    starlight({
      title: "DBCrab",
      description: "Documentation for the DBCrab PostgreSQL client.",
      logo: {
        src: "../docs/assets/logo.svg",
        alt: "DBCrab",
      },
      favicon: "/logo.svg",
      social: [{ icon: "gitlab", label: "GitLab", href: "https://gitlab.com/akhansari/dbcrab" }],
      sidebar: [
        {
          label: "Start Here",
          items: [{ slug: "installation" }, { slug: "quickstart" }, { slug: "connecting" }],
        },
        {
          label: "Guides",
          items: [
            { slug: "guides/sql-repl" },
            { slug: "guides/command-mode" },
            { slug: "guides/results-tui" },
            { slug: "guides/editing-results" },
            { slug: "guides/named-sql" },
            { slug: "guides/contexts-history" },
            { slug: "guides/csv-transfer" },
            { slug: "guides/agentic-process" },
            { slug: "guides/configuration" },
            { slug: "guides/keybindings" },
          ],
        },
        {
          label: "Reference",
          items: [
            { slug: "reference/cli" },
            { slug: "reference/commands-session-catalog" },
            { slug: "reference/commands-object-inspection" },
            { slug: "reference/commands-transfer" },
            { slug: "reference/named-sql" },
            { slug: "reference/configuration" },
            { slug: "reference/keybindings" },
            { slug: "reference/display-output" },
            { slug: "reference/paths-environment" },
          ],
        },
        {
          label: "Help",
          items: [{ slug: "troubleshooting" }],
        },
      ],
    }),
  ],
})
