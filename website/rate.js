// Mister Download Manager — the site's own star ratings (build.py rate_sheet; the Worker that
// keeps them is website/ratings/). The sheet shows this build's numbers; once it comes near, they
// are read again, with the visitor's own rating. A visitor picks one to five stars: a click sends
// at once, the keyboard moves along the stars (a radio group) and Enter or Space sends. Sending
// asks Cloudflare Turnstile for a token first (its script loads only once the visitor reaches for
// the stars; it asks the visitor something only when it must); the Worker counts one rating per
// visitor, so picking again changes theirs. The average and the bars follow, a stamp says thanks,
// and the status line (a live region) says it all in words. Reduced motion: everything at rest.
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

  var saved = null;   // the visitor's rating, as the Worker knows it
  var pending = null; // picked with the keyboard, not sent yet
  var hover = null;
  var resting = null; // the star just picked, still under the pointer
  var sending = false;
  var data = readSheet();

  try {
    var local = +localStorage.getItem(KEY);
    if (local >= 1 && local <= 5) saved = local;
  } catch (e) { /* none */ }

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

  // The average, its stars, the count and the bars, from the Worker's numbers.
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

  // The status line (a live region): `text` shown, `quiet` for screen readers only.
  function say(text, error, quiet) {
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
      input.addEventListener("change", function () {
        if (sending) return;
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
  // Turnstile's script comes only when the visitor reaches for the stars.
  function prepare() { widget().catch(function () { /* said when the visitor picks */ }); }
  pick.addEventListener("pointerenter", prepare, { once: true });
  pick.addEventListener("focusin", prepare, { once: true });

  function mark() {
    for (var i = 0; i < inputs.length; i++) inputs[i].checked = saved !== null && i + 1 === saved;
  }

  // ---- Turnstile -------------------------------------------------------------------------------

  var turnstileLoad = null, widgetId = null, waiting = null;
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
  function settle(fn, value) {
    if (!waiting) return;
    var w = waiting; waiting = null; clearTimeout(w.timer); w[fn](value);
  }
  function widget() {
    return loadTurnstile().then(function (ts) {
      if (widgetId === null) {
        widgetId = ts.render(check, {
          sitekey: sheet.getAttribute("data-sitekey"), action: "rate", appearance: "interaction-only", execution: "execute",
          theme: "auto", language: "auto", "refresh-expired": "manual",
          callback: function (token) { settle("resolve", token); },
          "error-callback": function () { settle("reject", { status: 403 }); return true; },
          "expired-callback": function () { settle("reject", { status: 403 }); },
          "timeout-callback": function () { settle("reject", { status: 403 }); },
        });
      }
      return ts;
    });
  }
  function token() {
    return widget().then(function (ts) {
      return new Promise(function (resolve, reject) {
        // A question Turnstile asks gets a minute; then the visitor can pick again.
        waiting = { resolve: resolve, reject: reject, timer: setTimeout(function () { settle("reject", { status: 403 }); }, 60000) };
        try { ts.reset(widgetId); } catch (e) { /* new */ }
        ts.execute(widgetId);
      });
    });
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

  function averageSaid() {
    return t("Average: {avg} out of 5, from {count}.").replace("{avg}", fmt(oneDecimal(data.average), 1)).replace("{count}", countText(data.count));
  }

  // The visitor takes their rating back.
  removeBtn.addEventListener("click", function () {
    if (sending || !saved) return;
    sending = true;
    pick.classList.add("is-sending");
    say(t("Sending…"));
    fetch(api, { method: "DELETE", credentials: "omit" }).then(answer).then(function (body) {
      sending = false;
      saved = null;
      pending = null;
      try { localStorage.removeItem(KEY); } catch (e) { /* none */ }
      pick.classList.remove("is-sending", "is-thanked");
      mark();
      paint();
      render(body);
      say(t("Your rating is removed."), false, averageSaid());
      stars[0].querySelector("input").focus();
    }).catch(function (err) {
      sending = false;
      pick.classList.remove("is-sending");
      failed(err);
    });
  });

  function send(n) {
    if (sending) return;
    if (hover) resting = n;
    hover = null;
    if (n === saved) {
      pending = null; mark(); paint();
      say(t("That is already your rating. Pick another number of stars to change it."));
      return;
    }
    var before = saved;
    sending = true;
    pending = n;
    pick.classList.add("is-sending");
    pick.classList.remove("is-thanked");
    paint();
    say(t("Sending…"));
    token().then(function (tok) {
      return fetch(api, {
        method: "POST", credentials: "omit", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ stars: n, turnstileToken: tok }),
      });
    }).then(answer).then(function (body) {
      sending = false;
      pending = null;
      saved = +body.mine || n;
      try { localStorage.setItem(KEY, String(saved)); } catch (e) { /* not kept */ }
      pick.classList.remove("is-sending");
      mark();
      paint();
      pop(saved);
      void pick.offsetWidth;
      pick.classList.add("is-thanked");
      render(body);
      say(before || body.previous ? t("Thank you! Your rating is changed.") : t("Thank you! Your rating is in."), false, averageSaid());
    }).catch(function (err) {
      sending = false;
      pending = null;
      pick.classList.remove("is-sending");
      mark();
      paint();
      failed(err);
    });
  }

  // ---- When the sheet comes near: fresh numbers; in view: the bars fill in ---------------------

  function refresh() {
    fetch(api, { credentials: "omit" }).then(function (res) {
      if (!res.ok) throw new Error(String(res.status));
      return res.json();
    }).then(function (body) {
      if (sending) return;
      saved = body.mine >= 1 && body.mine <= 5 ? +body.mine : null;
      try { saved ? localStorage.setItem(KEY, String(saved)) : localStorage.removeItem(KEY); } catch (e) { /* not kept */ }
      mark();
      paint();
      render(body);
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
