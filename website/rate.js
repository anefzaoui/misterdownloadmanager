// Mister Download Manager — the site's own star ratings (build.py rate_sheet; the Worker that
// keeps them is website/ratings/). The sheet shows this build's numbers; once it comes near, they
// are read again, with the visitor's own rating. A visitor picks one to five stars: a click sends
// at once, the keyboard moves along the stars (a radio group) and Enter or Space sends. Sending
// needs a token from Cloudflare Turnstile: its script loads, and its check starts in the
// background, only once the visitor reaches for the stars (it asks the visitor something only
// when it must), so the token is usually ready by the click; after each rating the next one is
// made the same way, for a change. A pick shows as given at once: the stars, a stamp saying
// thanks, the average and the bars with it counted. The Worker's answer then sets the numbers
// (it counts one rating per visitor, so picking again changes theirs); if the rating couldn't be
// sent, everything goes back as it was and says why. While Turnstile asks the visitor something,
// or after a try that failed, the stars wait for the answer instead. The status line (a live
// region) says it all in words. Reduced motion: everything at rest.
(function () {
  "use strict";
  var sheet = document.querySelector("[data-rate]");
  if (!sheet || !window.fetch || !window.Promise) return;
  var root = document.documentElement;
  var I18N = window.MDM_I18N || {};
  function t(s) { return I18N[s] || s; }
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var lang = root.getAttribute("lang") || "en";
  var api = sheet.getAttribute("data-api");
  var WORDS = [t("Poor"), t("Fair"), t("Good"), t("Very good"), t("Excellent")];
  var KEY = "mdm-rating";

  var pick = sheet.querySelector("[data-rate-pick]");
  var stars = pick.querySelectorAll(".rate-star");
  var inputs = pick.querySelectorAll("input[type=radio]");
  var word = pick.querySelector("[data-rate-word]");
  var hint = pick.querySelector(".rate-hint");
  var removeBtn = pick.querySelector("[data-rate-remove]");
  var status = pick.querySelector("[data-rate-status]");
  var check = pick.querySelector("[data-rate-check]");
  var avgNum = sheet.querySelector("[data-rate-avg] [data-num]");
  var avgStars = sheet.querySelector(".rate-stars-avg");
  var countLine = sheet.querySelector("[data-rate-count]");
  var some = sheet.querySelector("[data-rate-some]");
  var none = sheet.querySelector("[data-rate-none]");
  var rows = sheet.querySelectorAll("[data-rate-bars] li");
  var restWord = word.textContent;
  var restHint = hint.textContent;

  var data = readSheet(); // the numbers shown
  var confirmed = { mine: null, data: data }; // what the Worker said last (at first, this build)
  var desired = null; // the rating the visitor gave last: sent, on its way or to send
  var saved = null;   // the visitor's rating as the stars show it
  var pending = null; // picked with the keyboard, not sent yet
  var hover = null;
  var resting = null; // the star just picked, still under the pointer
  var flight = null;  // a rating on its way to the Worker
  var removing = false;
  var epoch = 0;      // counts sends, so an older reading of the numbers never shows over one

  try {
    var local = +localStorage.getItem(KEY);
    if (local >= 1 && local <= 5) confirmed.mine = desired = saved = local;
  } catch (e) { /* none */ }
  function keep(mine) {
    try { mine ? localStorage.setItem(KEY, String(mine)) : localStorage.removeItem(KEY); } catch (e) { /* not kept */ }
  }

  // ---- Numbers ---------------------------------------------------------------------------------

  var formats = {};
  function fmt(n, dec) {
    try {
      formats[dec] = formats[dec] || new Intl.NumberFormat(lang, { minimumFractionDigits: dec, maximumFractionDigits: dec, numberingSystem: "latn" });
      return formats[dec].format(n);
    } catch (e) {
      return n.toFixed(dec);
    }
  }
  function countText(n) { return n === 1 ? t("1 rating") : t("{n} ratings").replace("{n}", fmt(n, 0)); }
  function oneDecimal(x) { return Math.round(x * 10) / 10; }

  function readSheet() {
    var d = { average: avgNumber(), count: 0, distribution: {} };
    for (var i = 0; i < rows.length; i++) {
      var n = +rows[i].querySelector(".rate-bar-n [data-num]").getAttribute("data-num");
      d.distribution[rows[i].getAttribute("data-stars")] = n;
      d.count += n;
    }
    return d;
  }
  function avgNumber() {
    var el = sheet.querySelector("[data-rate-avg] [data-num]");
    return el ? +el.getAttribute("data-num") : 0;
  }

  // The numbers with one visitor's rating moved from `from` stars to `to` (either may be none).
  function project(base, from, to) {
    var dist = {}, count = 0, sum = 0;
    for (var s = 1; s <= 5; s++) dist[s] = +(base.distribution || {})[s] || 0;
    if (from) dist[from] = Math.max(0, dist[from] - 1);
    if (to) dist[to] += 1;
    for (s = 1; s <= 5; s++) { count += dist[s]; sum += s * dist[s]; }
    return { average: count ? sum / count : 0, distribution: dist };
  }

  // A number counting from what it shows to what it is now, damped (at once with reduced motion).
  function roll(el, from, to, dec) {
    el.setAttribute("data-num", to);
    if (reduce || from === to) { el.textContent = fmt(to, dec); return; }
    var start = performance.now(), length = 900;
    function step(now) {
      var k = Math.min(1, (now - start) / length);
      var eased = k === 1 ? 1 : 1 - Math.pow(2, -10 * k);
      el.textContent = fmt(from + (to - from) * eased, dec);
      if (k < 1) requestAnimationFrame(step);
    }
    requestAnimationFrame(step);
  }

  // The average, its stars, the count and the bars, from these numbers.
  function render(next) {
    var dist = next.distribution || {};
    var count = 0, top = 1;
    for (var s = 1; s <= 5; s++) { count += +dist[s] || 0; top = Math.max(top, +dist[s] || 0); }
    var avg = oneDecimal(+next.average || 0);
    some.hidden = !count;
    none.hidden = !!count;
    roll(avgNum, oneDecimal(data.average), avg, 1);
    avgStars.style.setProperty("--fill", (avg / 5 * 100).toFixed(1) + "%");
    countLine.textContent = countText(count);
    for (var i = 0; i < rows.length; i++) {
      var k = rows[i].getAttribute("data-stars");
      var n = +dist[k] || 0;
      rows[i].querySelector(".rate-bar-fill").style.setProperty("--share", (n / top).toFixed(4));
      roll(rows[i].querySelector(".rate-bar-n [data-num]"), +data.distribution[k] || 0, n, 0);
      rows[i].classList.toggle("is-mine", saved !== null && +k === saved);
    }
    data = { average: avg, count: count, distribution: dist };
  }

  // ---- The stars -------------------------------------------------------------------------------

  function setWord(text) {
    if (word.textContent === text) return;
    word.textContent = text;
    word.classList.remove("is-swap");
    void word.offsetWidth;
    word.classList.add("is-swap");
  }

  function paint() {
    var show = hover || pending || saved || 0;
    for (var i = 0; i < stars.length; i++) {
      var n = i + 1;
      stars[i].classList.toggle("is-preview", !!hover && n <= hover);
      stars[i].classList.toggle("is-pending", !hover && !!pending && n <= pending);
      stars[i].classList.toggle("is-on", !hover && !pending && saved !== null && n <= saved);
      stars[i].classList.toggle("is-target", !!hover && n === hover);
    }
    setWord(show ? WORDS[show - 1] : restWord);
    for (var r = 0; r < rows.length; r++) {
      rows[r].classList.toggle("is-mine", saved !== null && +rows[r].getAttribute("data-stars") === saved);
    }
    hint.textContent = saved ? t("Your rating: {n} out of 5. Pick again any time to change it.").replace("{n}", fmt(saved, 0)) : restHint;
    removeBtn.hidden = !saved;
  }

  // The stars, the bars and the numbers with `rating` as the visitor's: the Worker's numbers, the
  // visitor's rating moved to it while the Worker doesn't have it yet.
  function show(rating) {
    saved = rating;
    mark();
    paint();
    render(rating === confirmed.mine ? confirmed.data : project(confirmed.data, confirmed.mine, rating));
  }

  // The status line (a live region): `text` shown, `quiet` for screen readers only.
  var said = "";
  function say(text, error, quiet) {
    said = text + "\n" + (quiet || "");
    status.textContent = text;
    if (quiet) {
      var sr = document.createElement("span");
      sr.className = "sr-only";
      sr.textContent = " " + quiet;
      status.appendChild(sr);
    }
    status.classList.toggle("is-error", !!error);
  }

  function pop(n) {
    if (reduce) return;
    for (var i = 0; i < n; i++) {
      var star = stars[i];
      star.style.setProperty("--i", i);
      star.classList.remove("is-pop");
      void star.offsetWidth;
      star.classList.add("is-pop");
    }
    setTimeout(function () { for (var j = 0; j < stars.length; j++) stars[j].classList.remove("is-pop"); }, 1100);
  }

  // The stars pop and the stamp says thanks.
  function thank() {
    pop(saved);
    pick.classList.remove("is-thanked");
    void pick.offsetWidth;
    pick.classList.add("is-thanked");
  }

  function averageSaid() {
    return t("Average: {avg} out of 5, from {count}.").replace("{avg}", fmt(oneDecimal(data.average), 1)).replace("{count}", countText(data.count));
  }

  // Thanks in words; `again`: said even when the line already says it (a new pick).
  function thanks(changed, again) {
    var text = changed ? t("Thank you! Your rating is changed.") : t("Thank you! Your rating is in.");
    var quiet = averageSaid();
    if (again || said !== text + "\n" + quiet) say(text, false, quiet);
  }

  // The stars wait for the Worker's answer (a removal, or Turnstile asking something).
  function waits() { return removing || (!!flight && !flight.optimistic); }

  for (var i = 0; i < stars.length; i++) {
    (function (star, n) {
      var input = star.querySelector("input");
      // Pointing previews (not a finger: it would stick). The star just picked shows the rating
      // itself, not a preview, until the pointer moves to another.
      star.addEventListener("pointermove", function (e) {
        if (e.pointerType === "touch" || n === resting || n === hover) return;
        resting = null;
        hover = n; paint();
      });
      // A click (a mouse, a finger, a screen reader's activation) sends at once; the keyboard's
      // arrows also "click" the radio they move to (detail 0): that only marks it, Enter sends.
      input.addEventListener("click", function (e) {
        if (e.detail > 0 || e.pointerType) send(n);
      });
      // (A click's own change comes after it, with the rating already on its way: nothing to do.)
      input.addEventListener("change", function () {
        if (waits() || flight) return;
        pending = n === saved ? null : n;
        pick.classList.remove("is-thanked");
        paint();
        if (pending) say(t("Press Enter to give it {n} out of 5.").replace("{n}", fmt(n, 0)));
      });
      input.addEventListener("keydown", function (e) {
        if (e.key === "Enter" || e.key === " " || e.key === "Spacebar") {
          e.preventDefault();
          input.checked = true;
          send(n);
        }
      });
    })(stars[i], i + 1);
  }
  pick.querySelector("[data-rate-stars]").addEventListener("pointerleave", function () { hover = null; resting = null; paint(); });
  // Turnstile's script comes, and its check starts, only when the visitor reaches for the stars.
  function prepare() { if (!trouble) presolve(); }
  pick.addEventListener("pointerenter", prepare);
  pick.addEventListener("focusin", prepare);

  function mark() {
    for (var i = 0; i < inputs.length; i++) inputs[i].checked = saved !== null && i + 1 === saved;
  }

  // ---- Turnstile -------------------------------------------------------------------------------

  // A token lives 300 s and is good once: one older than 4.5 minutes is never sent.
  var FRESH_MS = 270000;
  var turnstileLoad = null, widgetId = null;
  var ready = null;     // a token made and not sent yet, and when
  var solving = null;   // the check under way
  var asking = false;   // Turnstile is asking the visitor something
  var trouble = false;  // the last check or rating failed: the next pick waits for its answer

  function loadTurnstile() {
    if (turnstileLoad) return turnstileLoad;
    turnstileLoad = new Promise(function (resolve, reject) {
      if (window.turnstile) { resolve(window.turnstile); return; }
      window.mdmTurnstileReady = function () { resolve(window.turnstile); };
      var s = document.createElement("script");
      s.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit&onload=mdmTurnstileReady";
      s.async = true;
      s.onerror = function () { turnstileLoad = null; reject({ status: "no-check" }); };
      document.head.appendChild(s);
    });
    return turnstileLoad;
  }
  function solved(err) {
    if (!solving) return;
    var s = solving; solving = null; clearTimeout(s.timer);
    if (err) { trouble = true; s.reject(err); } else s.resolve();
  }
  function widget() {
    return loadTurnstile().then(function (ts) {
      if (widgetId === null) {
        widgetId = ts.render(check, {
          sitekey: sheet.getAttribute("data-sitekey"), action: "rate", appearance: "interaction-only", execution: "execute",
          theme: "auto", language: "auto", "refresh-expired": "manual",
          callback: function (tok) { asking = false; ready = { token: tok, at: Date.now() }; solved(null); },
          "error-callback": function () { asking = false; solved({ status: 403 }); return true; },
          "expired-callback": function () { ready = null; solved({ status: 403 }); },
          "timeout-callback": function () { asking = false; solved({ status: 403 }); },
          "before-interactive-callback": asked,
          "after-interactive-callback": function () { asking = false; },
        });
      }
      return ts;
    });
  }
  // Turnstile's check (its script first, once): settles once a token is ready.
  function solve() {
    if (solving) return solving.promise;
    var s = solving = {};
    s.promise = new Promise(function (resolve, reject) { s.resolve = resolve; s.reject = reject; });
    widget().then(function (ts) {
      if (solving !== s) return;
      // A question Turnstile asks gets a minute; then the visitor can pick again.
      s.timer = setTimeout(function () { if (solving === s) solved({ status: 403 }); }, 60000);
      ready = null;
      try { ts.reset(widgetId); } catch (e) { /* new */ }
      ts.execute(widgetId);
    }, function (err) {
      if (solving !== s) return;
      solving = null;
      trouble = true;
      s.reject(err);
    });
    return s.promise;
  }
  function fresh() { return !!ready && Date.now() - ready.at < FRESH_MS; }
  // A token made in the background, so that a pick can go at once.
  function presolve() {
    if (!fresh() && !solving) solve().catch(function () { /* said when the visitor picks */ });
  }
  // The token to send: the one made in the background while it's fresh, else a new one.
  function token() {
    return (fresh() ? Promise.resolve() : solve()).then(function () {
      if (!fresh()) throw { status: 403 };
      var tok = ready.token;
      ready = null;
      return tok;
    });
  }
  // Turnstile asks the visitor something: a pick shown as given waits for it instead.
  function asked() {
    asking = true;
    if (flight && flight.optimistic && !flight.posted) {
      flight.optimistic = false;
      wait(desired);
    }
  }

  // ---- Sending ---------------------------------------------------------------------------------

  // The Worker's answer as JSON, or a thrown {status} when it refused.
  function answer(res) {
    return res.json().catch(function () { return {}; }).then(function (body) {
      if (!res.ok) throw { status: res.status };
      return body;
    });
  }

  function failed(err) {
    var code = err && err.status;
    say(code === "no-check" ? t("The check that you are a person couldn't start: a content blocker may be stopping challenges.cloudflare.com.")
      : code === 429 ? t("Too many tries from your connection. Please try again in a few minutes.")
      : code === 403 ? t("The check that you are a person didn't pass. Please try again.")
      : code === 503 ? t("Ratings are closed for a moment. Please try again later.")
      : t("Your rating couldn't be sent. Please try again later."), true);
  }

  // The visitor takes their rating back.
  removeBtn.addEventListener("click", function () {
    if (flight || removing || !saved) return;
    removing = true;
    epoch++;
    pick.classList.add("is-sending");
    say(t("Sending…"));
    fetch(api, { method: "DELETE", credentials: "omit" }).then(answer).then(function (body) {
      removing = false;
      confirmed = { mine: null, data: body };
      desired = null;
      pending = null;
      keep(null);
      pick.classList.remove("is-sending", "is-thanked");
      show(null);
      say(t("Your rating is removed."), false, averageSaid());
      stars[0].querySelector("input").focus();
    }).catch(function (err) {
      removing = false;
      pick.classList.remove("is-sending");
      failed(err);
    });
  });

  // The stars wait for the answer, as `n` (the look while Turnstile asks, or after a failed try).
  function wait(n) {
    show(confirmed.mine);
    pending = n;
    pick.classList.add("is-sending");
    pick.classList.remove("is-thanked");
    paint();
    say(t("Sending…"));
  }

  function send(n) {
    if (waits()) return;
    if (hover) resting = n;
    hover = null;
    if (n === saved) {
      pending = null; mark(); paint();
      say(t("That is already your rating. Pick another number of stars to change it."));
      return;
    }
    var changed = saved !== null;
    desired = n;
    pending = null;
    if (!flight && (asking || trouble)) {
      wait(n);
      fly(changed, false);
      return;
    }
    // Shown as given now. With a rating already on its way, this one goes when it is answered
    // (or in its place, if it hasn't left yet): one request at a time, the last pick last.
    show(n);
    thank();
    thanks(changed, true);
    if (!flight) fly(changed, true);
  }

  // Sends `desired` (read when its token is ready). `optimistic`: it shows as given already.
  function fly(changed, optimistic) {
    var f = flight = { optimistic: optimistic, changed: changed, posted: false, stars: desired };
    epoch++;
    removeBtn.disabled = true;
    token().then(function (tok) {
      f.posted = true;
      f.stars = desired;
      return fetch(api, {
        method: "POST", credentials: "omit", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ stars: f.stars, turnstileToken: tok }),
      });
    }).then(answer).then(function (body) {
      flight = null;
      trouble = false;
      var mine = +body.mine || f.stars;
      confirmed = { mine: mine, data: body };
      keep(mine);
      // The next token, made in the background, for a change of mind.
      setTimeout(presolve, 1500);
      if (desired !== mine) {
        show(desired);
        fly(true, true);
        return;
      }
      removeBtn.disabled = false;
      pending = null;
      pick.classList.remove("is-sending");
      show(mine);
      if (!f.optimistic) thank();
      thanks(f.changed || body.previous != null, !f.optimistic);
    }).catch(function (err) {
      flight = null;
      trouble = true;
      desired = confirmed.mine;
      removeBtn.disabled = false;
      pending = null;
      pick.classList.remove("is-sending", "is-thanked");
      show(confirmed.mine);
      failed(err);
    });
  }

  // ---- When the sheet comes near: fresh numbers; in view: the bars fill in ---------------------

  function refresh() {
    var since = epoch;
    fetch(api, { credentials: "omit" }).then(function (res) {
      if (!res.ok) throw new Error(String(res.status));
      return res.json();
    }).then(function (body) {
      if (flight || removing || since !== epoch) return;
      var mine = body.mine >= 1 && body.mine <= 5 ? +body.mine : null;
      confirmed = { mine: mine, data: body };
      desired = mine;
      keep(mine);
      show(mine);
    }).catch(function () { /* this build's numbers stay */ });
  }

  mark();
  paint();
  if ("IntersectionObserver" in window) {
    var near = new IntersectionObserver(function (entries) {
      if (entries[0].isIntersecting) { near.disconnect(); refresh(); }
    }, { rootMargin: "600px 0px" });
    near.observe(sheet);
    var seen = new IntersectionObserver(function (entries) {
      if (entries[0].isIntersecting) { seen.disconnect(); sheet.classList.add("is-seen"); }
    }, { threshold: 0.3 });
    seen.observe(sheet);
  } else {
    sheet.classList.add("is-seen");
    refresh();
  }
})();
