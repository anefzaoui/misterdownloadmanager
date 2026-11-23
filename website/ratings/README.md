# The site's star ratings

Visitors of misterdownloadmanager.com rate Mister Download Manager from one to five stars on the
home and download pages. This folder is the service that keeps those ratings: a Cloudflare Worker
(`src/worker.js`, `src/ratings.js`) over a D1 database (`migrations/`). The site's side is
`website/rate.js` (the widget), `tools/website/build.py` (`rate_sheet`, `rating_ld`) and
`tools/website/tallies.py` (the numbers baked in at each build).

- `GET /api/ratings` gives `{average, count, distribution, mine}` (`mine`: the asking visitor's
  own rating, or `null`).
- `POST /api/ratings` with `{stars, turnstileToken}` rates (or changes the visitor's rating).
- `DELETE /api/ratings` removes the visitor's rating.

One rating per visitor: a visitor is an HMAC-SHA256 of their IP address with a secret salt (an
IPv6 address by its /64). The address itself is never stored or logged, and the Worker turns
Cloudflare's request logs off (`[observability] enabled = false`). A rating needs a Cloudflare
Turnstile token, checked by the Worker; each visitor gets 8 ratings or removals in 10 minutes;
only the site's own pages may call it (CORS, `ALLOWED_ORIGINS`); stars are whole numbers from 1
to 5. The site's privacy page says the same in plain words: change both together.

Until the Worker is deployed and the site knows its address, the site shows no rating widget and
puts no rating in its structured data; the build works as before.

## Try it on your computer

No Cloudflare account is needed. Node.js 22.13 or newer:

```
cd website/ratings
npm ci
npm test                          # the Worker against SQLite, Turnstile stood in for
cp .dev.vars.example .dev.vars    # local salt, Turnstile's always-pass test secret
npx wrangler d1 migrations apply mdm-ratings --local
npm run dev                       # http://127.0.0.1:8787/api/ratings
```

Then, in another terminal, build the site against it with Turnstile's public test site key
(always passes):

```
MDM_RATINGS_API=http://127.0.0.1:8787/api/ratings MDM_TURNSTILE_SITEKEY=1x00000000000000000000AA python tools/website/build.py --serve
```

(PowerShell: set `$env:MDM_RATINGS_API` and `$env:MDM_TURNSTILE_SITEKEY` first.) Ratings made
against a local Worker are never written to `website/content/ratings.json`. Turnstile's other
test keys: site key `2x00000000000000000000AB` always blocks, `3x00000000000000000000FF` always
asks the visitor; secret `2x0000000000000000000000000000000AA` always fails. Set
`WRANGLER_SEND_METRICS=false` to keep Wrangler from sending its usage statistics.

## Deploy it (the maintainer, once)

Everything below fits Cloudflare's free plan (Workers: 100,000 requests a day; D1: 5 million
rows read and 100,000 written a day; Turnstile: free). Reading the numbers reads six rows.

### 1. In Cloudflare's dashboard

1. Create a free account at [dash.cloudflare.com](https://dash.cloudflare.com/sign-up), or use
   the one misterdownloadmanager.com's DNS is in. Note the **Account ID** (Account home, the
   **⋯** next to the account's name, or the right column of any domain's Overview).
2. **Workers & Pages**: open it once, so the account has Workers. The Worker answers only at
   `ratings.misterdownloadmanager.com` (`routes` in `wrangler.toml`, a Custom Domain): Cloudflare
   makes that host name's DNS record and certificate itself. It has no address on Cloudflare's
   shared Workers domain and no preview addresses (`workers_dev` and `preview_urls` are off).
3. **Turnstile → Add widget**: name it "MDM ratings", add the host names
   `misterdownloadmanager.com` and `www.misterdownloadmanager.com`, widget mode **Managed**,
   no pre-clearance. Copy its **Site key** (public) and **Secret key** (secret: never in the
   repository).
4. To deploy from the command line, either sign in with `npx wrangler login` (a browser window)
   or create an API token: **My Profile → API Tokens → Create Token → Custom token**, with only
   these permissions, for this account only:
   - Account · **Workers Scripts** · Edit
   - Account · **D1** · Edit
   - Account · **Account Settings** · Read

   Then set `CLOUDFLARE_API_TOKEN` (the token) and `CLOUDFLARE_ACCOUNT_ID` in the terminal you
   deploy from. Keep the token out of the repository; delete it when done if you won't deploy
   again soon.

### 2. From `website/ratings/`

```
npm ci
npx wrangler d1 create mdm-ratings
```

Copy the `database_id` it prints into `wrangler.toml` (in place of the zeros), then:

```
npx wrangler d1 migrations apply mdm-ratings --remote
npx wrangler secret put IP_SALT            # paste a long random value (below)
npx wrangler secret put TURNSTILE_SECRET   # paste Turnstile's Secret key
npx wrangler deploy
```

A salt: `node -e "console.log(require('crypto').randomBytes(32).toString('base64url'))"`. Keep
it only in Cloudflare (the secret); a new salt later makes every visitor new, so every rating
could be given again.

The first deploy attaches the Custom Domain. A token with only the three permissions above may
not be allowed to create it: then add it once in the dashboard (**Workers & Pages → mdm-ratings →
Settings → Domains & Routes → Add → Custom domain**, `ratings.misterdownloadmanager.com`) and
deploy again. Check it:

```
curl https://ratings.misterdownloadmanager.com/api/ratings
```

answers `{"average":0,"count":0,"distribution":{...},"mine":null}`.

### 3. Turn it on in the site

In `tools/website/build.py`, `RATINGS_API_LIVE` is the Worker's address
(`https://ratings.misterdownloadmanager.com/api/ratings`) and `TURNSTILE_SITEKEY_LIVE` the
Turnstile widget's Site key (public). With both set, the home and download pages show the
ratings sheet, each build reads the numbers again (and keeps the last known ones in
`website/content/ratings.json`), and the structured data carries them once there is a rating.

## Later

- Code changes: `npm test`, try it locally as above, then `npx wrangler deploy`.
- A new table or column: a new file in `migrations/` (`0002_….sql`), applied with
  `npx wrangler d1 migrations apply mdm-ratings --remote` before deploying the code that uses it.
- Wrangler is pinned in `package.json` and `package-lock.json`; update it on purpose, then test.
