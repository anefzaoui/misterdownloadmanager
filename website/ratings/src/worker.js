// The Worker's entry point. Its module may export only handlers (Cloudflare reads every export
// as one), so the ratings themselves, the site's live figures, and what the tests check, are in
// ratings.js and stats.js.
import { respond } from "./ratings.js";
import { STATS_PATH, respondStats } from "./stats.js";

export default {
  fetch(request, env, ctx) {
    if (new URL(request.url).pathname === STATS_PATH) return respondStats(request, env, ctx);
    return respond(request, env);
  },
};
