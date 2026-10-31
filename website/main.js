// Mister Download Manager — site behaviour: light/dark, the download for your system, the
// split button's menu, the "What do you download?" chips and the rotating hello.
(function () {
  "use strict";
  var root = document.documentElement;

  function each(selector, fn) {
    var els = document.querySelectorAll(selector);
    for (var i = 0; i < els.length; i++) fn(els[i]);
  }

  // ---- Light / dark ------------------------------------------------------------------------
  // The page follows the system until the visitor picks; the pick is remembered.
  var media = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  function current() {
    var t = root.getAttribute("data-theme");
    if (t === "light" || t === "dark") return t;
    return media && media.matches ? "dark" : "light";
  }
  each(".theme", function (button) {
    button.addEventListener("click", function () {
      var next = current() === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem("mdm-theme", next); } catch (e) { /* not remembered */ }
    });
  });

  // ---- The split button's menu: one open at a time, closed by a click elsewhere or Escape ----
  each(".split-more, .menu", function (d) {
    d.addEventListener("toggle", function () {
      if (d.open) each(".split-more, .menu", function (o) { if (o !== d) o.open = false; });
    });
  });
  document.addEventListener("click", function (e) {
    each(".split-more[open], .menu[open]", function (d) { if (!d.contains(e.target)) d.open = false; });
  });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") each(".split-more[open], .menu[open]", function (d) { d.open = false; d.querySelector("summary").focus(); });
  });

  // ---- The benchmark, replayed in real time ------------------------------------------------
  // Medians from docs/bench/run5/REPORT.md: startup (command to first request), transfer and
  // finish, in milliseconds. Each lane's eight parts fill as the connections would; a lane
  // whose server allows two connections fills two parts at a time.
  var RACES = [
    { label: "faster", note: "1 GiB over a fast link. IDM spends 3 s before its first request reaches the server.",
      mdm: [14, 438, 556], idm: [3076, 1831, 545], e2e: [1009, 5456], ratio: 5.41, conns: [8, 8] },
    { label: "faster", note: "A 256 KiB file. MDM is done in 30 ms; IDM is still starting.",
      mdm: [14, 7, 8], idm: [3090, 25, 32], e2e: [30, 3141], ratio: 105.9, conns: [4, 4] },
    { label: "faster", note: "A server that allows two connections. MDM learns it and keeps to two; IDM opened 480.",
      mdm: [14, 12546, 9], idm: [3069, 12733, 48], e2e: [12568, 15864], ratio: 1.26, conns: [2, 2] }
  ];
  each("[data-race]", function (box) {
    var lanes = { mdm: box.querySelector('[data-lane="mdm"]'), idm: box.querySelector('[data-lane="idm"]') };
    var tabs = box.querySelectorAll("[data-scenario]");
    var ratioEl = box.querySelector("[data-ratio]"), noteEl = box.querySelector("[data-note]");
    var frame = 0, current = 0;
    function total(t) { return t[0] + t[1] + t[2]; }
    // The phases' medians don't add up exactly to the end-to-end medians: scale them so the
    // finish times are the report's.
    RACES.forEach(function (r) {
      ["mdm", "idm"].forEach(function (k, n) {
        var f = r.e2e[n] / total(r[k]);
        r[k] = r[k].map(function (v) { return v * f; });
      });
    });
    function fmt(ms) { return ms < 1000 ? Math.round(ms) + " ms" : (ms / 1000).toFixed(2) + " s"; }
    function paint(lane, t, conns, now) {
      var parts = lane.querySelectorAll(".lane-bar i");
      var start = t[0], xfer = t[1], all = total(t);
      var p = Math.max(0, Math.min(1, (now - start) / xfer));
      for (var i = 0; i < parts.length; i++) {
        // Connections work on `conns` parts at a time; each part in its turn.
        var wave = Math.floor(i / conns), waves = Math.ceil(parts.length / conns);
        var v = Math.max(0, Math.min(1, p * waves - wave));
        parts[i].style.setProperty("--p", v.toFixed(3));
      }
      var done = now >= all;
      lane.classList.toggle("waiting", now > 0 && now < start);
      lane.classList.toggle("done", done);
      lane.querySelector("[data-time]").textContent = fmt(Math.min(now, all));
      lane.querySelector("[data-state]").textContent = done ? "Done" : now < start ? "Starting…" : now < start + xfer ? "Downloading" : "Finishing";
    }
    function run(k) {
      cancelAnimationFrame(frame);
      current = k;
      var r = RACES[k];
      for (var i = 0; i < tabs.length; i++) tabs[i].setAttribute("aria-selected", i === k ? "true" : "false");
      var end = Math.max(total(r.mdm), total(r.idm));
      // The report's ratio of medians (docs/bench/run5/REPORT.md).
      var ratio = r.ratio;
      noteEl.textContent = r.note;
      ratioEl.textContent = "1.0×";
      var t0 = performance.now();
      (function tick(now) {
        var ms = now - t0;
        paint(lanes.mdm, r.mdm, r.conns[0], ms);
        paint(lanes.idm, r.idm, r.conns[1], ms);
        // The ratio grows as IDM keeps going after MDM has finished.
        var shown = ms >= end ? ratio : ms <= total(r.mdm) ? 1 : Math.min(ratio, ms / total(r.mdm));
        ratioEl.textContent = (shown >= 10 ? Math.round(shown) : shown < 2 && ms >= end ? shown.toFixed(2) : shown.toFixed(1)) + "×";
        if (ms < end) frame = requestAnimationFrame(tick);
        else ratioEl.textContent = (ratio >= 10 ? Math.round(ratio) : ratio < 2 ? ratio.toFixed(2) : ratio.toFixed(1)) + "×";
      })(t0);
    }
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].addEventListener("click", function () { run(+this.getAttribute("data-scenario")); });
    }
    box.querySelector("[data-race-replay]").addEventListener("click", function () { run(current); });
    // Start when it comes into view; at once where the browser can't tell.
    if ("IntersectionObserver" in window) {
      var seen = new IntersectionObserver(function (es) {
        if (es[0].isIntersecting) { seen.disconnect(); run(0); }
      }, { threshold: 0.4 });
      seen.observe(box);
    } else run(0);
  });

  // ---- Hello in the app's languages: each word written in, its chip lit -----------------------
  var stage = document.querySelector("[data-hello]");
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (stage && !reduce) {
    var words = [["Hello", "en"], ["مرحبا", "ar"], ["Bonjour", "fr"], ["Hallo", "de"], ["Hola", "es"],
      ["こんにちは", "ja"], ["Olá", "pt"], ["Привет", "ru"], ["नमस्ते", "hi"], ["Merhaba", "tr"], ["你好", "zh"]];
    var chips = document.querySelectorAll("[data-hello-chips] [lang]");
    var n = 0;
    setInterval(function () {
      var old = stage.querySelector('[data-pos="current"]');
      if (old) {
        old.setAttribute("data-pos", "past");
        setTimeout(function () { if (old.parentNode) old.parentNode.removeChild(old); }, 450);
      }
      n = (n + 1) % words.length;
      var word = document.createElement("span");
      word.className = "hello-word";
      word.setAttribute("data-pos", "current");
      word.setAttribute("lang", words[n][1]);
      word.textContent = words[n][0];
      stage.appendChild(word);
      for (var i = 0; i < chips.length; i++) {
        if (chips[i].getAttribute("lang") === words[n][1]) chips[i].setAttribute("data-on", "");
        else chips[i].removeAttribute("data-on");
      }
    }, 2600);
  }

  // ---- Your system -------------------------------------------------------------------------
  // Each system's card (.os-card[data-os]) carries its main file and label; the download
  // buttons take the visitor's, and the cards mark it. Phones and tablets keep Windows and see
  // that it's a desktop app.
  function detect() {
    var uad = navigator.userAgentData;
    var platform = ((uad && uad.platform) || navigator.platform || "").toLowerCase();
    var ua = (navigator.userAgent || "").toLowerCase();
    if (/android|iphone|ipad|ipod|mobile/.test(ua) || (uad && uad.mobile)) return "mobile";
    // iPadOS reports a Mac; a touch screen gives it away.
    if (platform.indexOf("mac") === 0 && navigator.maxTouchPoints > 1) return "mobile";
    if (platform.indexOf("win") === 0 || ua.indexOf("windows") >= 0) return "windows";
    if (platform.indexOf("mac") === 0 || ua.indexOf("mac os") >= 0) return "macos";
    if (/linux|x11|cros|bsd/.test(platform + " " + ua)) return "linux";
    return "windows";
  }

  var os = detect();
  if (os === "mobile") {
    each("[data-mobile-note]", function (el) { el.hidden = false; });
    return;
  }
  var card = document.querySelector('.os-card[data-os="' + os + '"]');
  each(".os-card", function (el) { el.classList.toggle("current", el === card); });
  if (!card) return;
  each("[data-os-cta]", function (el) { el.setAttribute("href", card.getAttribute("data-href")); });
  each("[data-os-cta-icon]", function (el) { el.setAttribute("href", "#os-" + os); });
  each("[data-os-label]", function (el) { el.textContent = card.getAttribute("data-label"); });
  // The detected system's card comes first.
  if (card.parentNode && card.parentNode.firstElementChild !== card) {
    card.parentNode.insertBefore(card, card.parentNode.firstElementChild);
  }
})();
