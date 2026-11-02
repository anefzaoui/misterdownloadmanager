// Mister Download Manager's link watcher (a content script, in every page and frame): a link
// clicked with Alt held is the browser's to download (background.js leaves its download alone),
// the key the app's help names for it.
(() => {
  if (globalThis.__mdmPage) return;
  globalThis.__mdmPage = true;
  const ext = globalThis.browser ?? globalThis.chrome;
  const note = (e) => {
    if (!e.altKey || !e.isTrusted || !(e.target instanceof Element)) return;
    const a = e.target.closest("a[href]");
    if (!a || !/^https?:/i.test(a.href)) return;
    try {
      ext.runtime.sendMessage({ type: "bypass", url: a.href }).catch(() => {});
    } catch {
      // The extension was updated or removed since this page loaded: nothing to tell.
    }
  };
  addEventListener("click", note, true);
  addEventListener("auxclick", note, true);
})();
