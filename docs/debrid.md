# Debrid service accounts

A debrid service, such as Real-Debrid, is a paid account that downloads files from file hosts
on its own servers. It hands back a plain direct link, without the host's waits, speed caps
and captchas. With your account in MDM, a link from a host your service supports is unlocked
through the service, then downloaded by MDM like any file: all its connections, pause and
resume.

Real-Debrid works now. AllDebrid and Premiumize come next.

> **Experimental:** built from Real-Debrid's public API documentation, not yet confirmed with a
> real account. If something doesn't work with your account, please
> [open an issue](https://github.com/anefzaoui/misterdownloadmanager/issues) (never paste your
> token in it).

## Add your account

1. **Options → Accounts → Add...**
2. Pick the service, then either:
   - **Sign in in the browser**: MDM shows a code and opens the service's page. Type the code
     there and allow MDM. The dialog notices on its own.
   - or paste your **API token** (**Find my token** opens the service's page that shows it),
     then **Use the token**.
3. MDM checks the account and shows its name and whether it is premium. **OK** keeps it.

The list's order is the order accounts are asked: a link goes through the first account
whose service supports its host. **Up** and **Down** change it.

**Download supported file-host links through these accounts** turns it all off and on.

## What happens to a link

- Add URL, the clipboard, the browser's **Download with Mister Download Manager** link menu,
  and batch downloads go through the account when its service supports
  the link's host.
- A download the browser had already started (MDM took it over from the browser) is
  downloaded as it is: the host is already sending the file.
- The list keeps the host's link. The direct link is never shown or copied: the services
  forbid sharing it. It lives only in the download's resume data, and when it expires MDM asks
  the service for a new one.
- A link the service can't give says why in the list: sign in again, premium needed, the host
  is down (MDM tries again later), the file is gone, and so on.

## Your account, your rules

- Use only your own account. Sharing an account or its links breaks the services' terms and
  can get the account suspended.
- MDM sends the service only the links you download through it, and your account's token.
  Nothing else.
- MDM ships no list of hosts or links; the service says which hosts it supports.

## Where the sign-in is kept

- **Windows**: sealed for your Windows account (DPAPI), in MDM's settings. A copy of the
  settings on another account or computer can't read it: sign in again there.
- **Linux and macOS**: in MDM's settings, in a data folder only you may read (0700), not yet
  in the system's keychain. Keeping it in the keychain is planned.

MDM never writes the token to its event log, crash reports, the automation API or the
command line's output.
