// Ready-made keys for services agents often call (docs/specs/secrets-consent.md):
// the person picks one and pastes the value; the rest is filled in.

import type { SecretInfo } from "../lib/protocol";

export type KeyPreset = {
  id: string;
  /** The service, as people call it. */
  label: string;
  secret: SecretInfo;
  /** Where the person makes a token. */
  url: string;
  /** The narrowest token that service offers. */
  hint: string;
};

const bearer = (name: string, host: string, variable: string): SecretInfo => ({
  name,
  host,
  header: "Authorization",
  prefix: "Bearer ",
  variable,
});

export const PRESETS: KeyPreset[] = [
  {
    id: "github",
    label: "GitHub",
    secret: bearer("github", "api.github.com", "GH_TOKEN"),
    url: "https://github.com/settings/personal-access-tokens/new",
    hint: "Make a fine-grained token for only the repositories this project needs.",
  },
  {
    id: "gitlab",
    label: "GitLab",
    secret: bearer("gitlab", "gitlab.com", "GITLAB_TOKEN"),
    url: "https://gitlab.com/-/user_settings/personal_access_tokens",
    hint: "Give the token only the scopes this project needs, such as read_api.",
  },
  {
    id: "stripe",
    label: "Stripe",
    secret: bearer("stripe", "api.stripe.com", "STRIPE_API_KEY"),
    url: "https://dashboard.stripe.com/test/apikeys",
    hint: "Use a test-mode or restricted key, never your live secret key.",
  },
  {
    id: "npm",
    label: "npm",
    secret: bearer("npm", "registry.npmjs.org", "NPM_TOKEN"),
    url: "https://www.npmjs.com/settings/~/tokens",
    hint: "Make a granular token for only the packages this project needs.",
  },
  {
    id: "huggingface",
    label: "Hugging Face",
    secret: bearer("huggingface", "huggingface.co", "HF_TOKEN"),
    url: "https://huggingface.co/settings/tokens",
    hint: "Make a fine-grained or read-only token.",
  },
];

/** "Other API": the person fills in everything. */
export const OTHER: SecretInfo = { name: "", host: "", header: "Authorization", prefix: "Bearer " };
