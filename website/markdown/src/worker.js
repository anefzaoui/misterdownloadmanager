// The Worker's entry point. Its module may export only handlers (Cloudflare reads every export
// as one), so the work itself, and what the tests check, is in markdown.js.
import { handle } from "./markdown.js";

export default {
  async fetch(request, env, ctx) {
    // An exception anywhere sends the request on to the site as if this Worker weren't there.
    ctx.passThroughOnException();
    try {
      return await handle(request);
    } catch {
      return fetch(request);
    }
  },
};
