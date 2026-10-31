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

  // ---- "What do you download?": tabs switch the words and the screenshot, and move on by
  // themselves (data-auto) when a tab's line has filled; hover or focus pauses them.
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  each("[data-picker]", function (picker) {
    var tabs = Array.prototype.slice.call(picker.querySelectorAll('[role="tab"]'));
    var auto = picker.hasAttribute("data-auto") && !still;
    function show(tab) {
      tabs.forEach(function (t) {
        var on = t === tab;
        var id = t.getAttribute("aria-controls");
        t.setAttribute("aria-selected", on ? "true" : "false");
        t.tabIndex = on ? 0 : -1;
        var panel = document.getElementById(id);
        if (panel) panel.hidden = !on;
        // One screenshot per kind (two where it has a day and a night version).
        var shots = picker.querySelectorAll('[data-for="' + id + '"]');
        for (var j = 0; j < shots.length; j++) shots[j].hidden = !on;
      });
      // Restart the selected tab's line.
      var bar = tab.querySelector(".job-bar i");
      if (bar) { bar.style.animation = "none"; void bar.offsetWidth; bar.style.animation = ""; }
    }
    function next(from) { return tabs[(tabs.indexOf(from) + 1) % tabs.length]; }
    tabs.forEach(function (tab) {
      tab.addEventListener("click", function () { show(tab); });
      tab.addEventListener("keydown", function (e) {
        var k = tabs.indexOf(tab);
        var to = e.key === "ArrowRight" ? tabs[(k + 1) % tabs.length]
          : e.key === "ArrowLeft" ? tabs[(k - 1 + tabs.length) % tabs.length] : null;
        if (to) { e.preventDefault(); show(to); to.focus(); }
      });
      var bar = tab.querySelector(".job-bar i");
      if (bar && auto) {
        bar.addEventListener("animationend", function () {
          if (tab.getAttribute("aria-selected") === "true") show(next(tab));
        });
      }
    });
    if (auto) {
      picker.classList.add("is-auto");
      picker.addEventListener("mouseenter", function () { picker.classList.add("is-paused"); });
      picker.addEventListener("mouseleave", function () { picker.classList.remove("is-paused"); });
      picker.addEventListener("focusin", function () { picker.classList.add("is-paused"); });
      picker.addEventListener("focusout", function () { picker.classList.remove("is-paused"); });
    }
    show(tabs[0]);
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
