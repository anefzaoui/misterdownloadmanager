// The Worker's entry point. Its module may export only handlers (Cloudflare reads every export
// as one), so the ratings themselves, and what the tests check, are in ratings.js.
import { respond } from "./ratings.js";

export default { fetch: respond };
