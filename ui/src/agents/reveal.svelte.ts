// A session another view (History) asks the Agents panel to open. The panel
// opens it once the session is among its project's and then clears the ask.

import type { SessionId } from "../lib/protocol";

class Reveal {
  session = $state<SessionId | null>(null);
}

export const reveal = new Reveal();
