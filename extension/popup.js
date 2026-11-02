// The toolbar button's popup: whether the browser reaches Mister Download Manager (and, when it
// doesn't, a link to the help page's section for why), pausing capture, leaving the current
// site to the browser, and opening the app or its Options. background.js does the work.
const ext = globalThis.browser ?? globalThis.chrome;
const HELP = "https://misterdownloadmanager.com/help/browser-help.html";
const t = (key, ...subs) => ext.i18n.getMessage(key, subs) || key;
const $ = (id) => document.getElementById(id);
const ask = (m) => ext.runtime.sendMessage(m).catch(() => ({ ok: false }));

document.documentElement.lang = ext.i18n.getUILanguage();
document.documentElement.dir = t("@@bidi_dir") === "rtl" ? "rtl" : "ltr";
for (const el of document.querySelectorAll("[data-i18n]")) el.textContent = t(el.dataset.i18n);
$("help").href = HELP;

async function show() {
  const s = await ask({ type: "state" });
  const box = $("status");
  const text = $("status-text");
  box.classList.remove("ok", "bad", "warn");
  if (s.connected && s.why === "update-app") {
    box.classList.add("warn");
    text.textContent = t("status_update_app");
  } else if (s.connected) {
    box.classList.add(s.paused ? "warn" : "ok");
    text.textContent = s.paused ? t("status_paused") : s.app ? t("status_connected_version", s.app) : t("status_connected");
  } else {
    box.classList.add("bad");
    text.textContent = t(`status_${(s.why || "not_installed").replace(/-/g, "_")}`);
  }
  const fix = $("fix");
  fix.hidden = s.connected && s.why !== "update-app";
  fix.href = `${HELP}#${s.why || "not-installed"}`;
  $("take").checked = !s.paused;
  $("take").disabled = !s.connected;
  $("open").disabled = $("options").disabled = !s.connected;
  $("version").textContent = s.version ? `v${s.version}` : "";
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true }).catch(() => []);
  let site = "";
  try {
    const u = new URL(tab && tab.url);
    if (/^https?:$/.test(u.protocol)) site = u.hostname;
  } catch {
    // Not a web page (the browser's own): nothing to leave.
  }
  const exclude = $("exclude");
  exclude.hidden = !site || !s.connected;
  exclude.textContent = t("popup_exclude_site", site);
  exclude.title = exclude.textContent;
  exclude.onclick = async () => {
    const r = await ask({ type: "exclude-site", host: site });
    if (r.ok) {
      exclude.hidden = true;
      $("excluded").hidden = false;
      $("excluded").textContent = t("popup_excluded", site);
    }
  };
}

$("take").addEventListener("change", async (e) => {
  await ask({ type: "pause", paused: !e.target.checked });
  show();
});
$("open").addEventListener("click", async () => {
  await ask({ type: "open-app" });
  window.close();
});
$("options").addEventListener("click", async () => {
  await ask({ type: "open-options" });
  window.close();
});

show();
