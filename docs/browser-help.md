# The browser and MDM aren't connected

[← Back to docs index](README.md)

When the browser can't hand a download to MDM, it keeps the download itself, so nothing is lost. The extension's toolbar button then shows a red **!**. Click it to see what's wrong, then **How to fix it** brings you to the matching section below.

To check everything at once, open **Options → General** in MDM and click **Test browser integration**. It runs each check below in turn and tells you the first one that fails.

## MDM isn't installed, or browser integration isn't set up {#not-installed}

The browser can't find the small helper program it talks to. It is registered when MDM is installed.

1. Install MDM with the setup from [the download page](https://misterdownloadmanager.com/download.html), and keep **Set up browser integration** ticked.
2. Already installed? Open MDM, then **Options → General → Test browser integration**, and click **Set it up** if it offers to.
3. The portable copy can't be connected: it writes nothing outside its own folder. Use the installed copy for browser integration. See [Portable mode](portable.md).
4. Restart the browser after setting it up.

## MDM didn't start in time {#not-running}

When MDM isn't running, the browser starts it for each download and waits for it. It waited 20 seconds this time and gave up.

- Start MDM yourself and try the download again.
- A first start, a slow disk, or an antivirus scanning MDM can take that long. Next time it is usually quick.
- If MDM never opens, see [The setup or MDM won't start](troubleshooting.md).

## The connection to MDM failed {#host-failed}

The browser started MDM's helper, but it stopped before answering. Security software is the usual cause.

- Allow `misterdownloadmanager-nmhost.exe` (in MDM's folder) in your antivirus or anti-malware program.
- With a VPN or proxy program, allow connections to `127.0.0.1` (your own computer): MDM's helper uses one to reach MDM.
- Reinstall MDM to restore its files.

## MDM doesn't accept this copy of the extension {#forbidden}

MDM's helper serves only its own extension. The browser says this copy isn't one it knows.

- **Two copies installed?** Open the browser's extensions page (`chrome://extensions`, `edge://extensions`, `brave://extensions`) and remove the one marked **Unpacked** (a copy loaded by hand), keeping the one from the store.
- **From another browser's store?** Install it from the link in MDM's **Add Mister Download Manager to your browser** window instead.
- **An older MDM?** Update MDM: newer extensions may be listed under IDs an old helper doesn't know.

## The extension is newer than MDM {#update-app}

Downloads still work, but some of the extension's features need a newer MDM. Update it with **Help → Check for updates** in MDM.

## A download's details were too large {#too-large}

A download carries its site's cookies to MDM. A few sites set so many that they exceed what the browser may send. The browser kept this one. Signing out and back in to the site usually trims them.

## Downloads stay with the browser although MDM is connected {#options}

The extension is connected, but MDM's own settings leave this download to the browser. Check, in MDM:

1. **Options → General → Use advanced browser integration** is on, and your browser is ticked under **Capture downloads from the following browsers**.
2. The file's type is listed in **Options → File types**.
3. The site isn't in **Don't start downloading automatically from the following sites**, nor the address in **Don't start downloading automatically from the following addresses**.

Also check the extension's toolbar button: with **Take downloads from this browser** off, everything stays with the browser.

## The browser opens PDFs and music instead of sending them {#opened}

MDM also takes the file types the browser would show or play itself (a PDF, an MP3), when they're listed in **Options → File types** and **Options → General → Also take files the browser would open** is on. Chrome 128 or later, Edge 128 or later, and Firefox are needed for this. To let the browser open one, hold **Alt** while clicking its link.

## The extension can't read this site {#site-access}

In Chrome, Edge and Brave you can limit an extension to some sites (right-click its toolbar button → **This can read and change site data**). On a site it may not read, the extension can't pass your sign-in on, so it leaves the download to the browser. Choose **On all sites** to let MDM take downloads everywhere.

## Firefox forgets the extension after a restart {#firefox-temporary}

An add-on loaded with **Load Temporary Add-on** is removed when Firefox closes. Install the signed add-on instead, from the link in MDM's **Add Mister Download Manager to your browser** window, or from the release's `-extension-firefox.xpi` file (open it in Firefox).

## Linux: Flatpak and Snap browsers {#linux-sandbox}

Browsers installed as a Flatpak or a Snap run in a sandbox that can't start programs outside it, so they can't reach MDM. Install the browser from your distribution's packages (or the browser maker's own `.deb` or `.rpm`) to use browser integration.

## Next steps

- [Browser integration →](browser-integration.md)
- [Troubleshooting →](troubleshooting.md)
- [Back to the docs index →](README.md)
