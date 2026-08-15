# Chrome Web Store listing: OpenCLI

This file is the source of truth for the first Store listing. Keep it synchronized with the extension behavior and [PRIVACY.md](../PRIVACY.md).

## URLs

| Store field | Value |
|---|---|
| Homepage | `https://github.com/forechoandlook/opencli_rs_plus` |
| Support URL | `https://github.com/forechoandlook/opencli_rs_plus/issues` |
| Privacy policy | `https://github.com/forechoandlook/opencli_rs_plus/blob/main/PRIVACY.md` |

The privacy-policy URL becomes valid after `PRIVACY.md` is committed and pushed to the repository's default branch. Do not submit the listing until it is public.

## Listing text

### Summary

```text
Run OpenCLI commands in your signed-in browser session through a local browser-to-CLI bridge.
```

### Detailed description

```text
OpenCLI connects the OpenCLI command-line tool to the Chrome profile you choose, so user-initiated commands can work with the websites where you are already signed in.

Use OpenCLI to run supported website data commands, inspect the current page's available actions, and save command results or requested downloads locally. The extension communicates only with the OpenCLI daemon running on your own computer; it does not use a cloud relay or analytics service.

What the extension does
- Runs OpenCLI commands in browser tabs created or selected for those commands.
- Uses your existing website session only when a command needs it.
- Shows the local daemon connection status and current-page actions in the extension popup.
- Saves output only when you ask OpenCLI to download it.

Permissions are used only for these functions. OpenCLI may access page content, tab information, and the relevant site's authentication cookies when needed for a command you initiate. It does not sell, rent, or share browser data with third parties. Read the privacy policy for details.
```

### Category and language

- Category: **Developer Tools**
- Primary language: **English**

## Store assets

| Store field | File | Notes |
|---|---|---|
| Extension icon | `extension/icons/icon-128.png` | Existing 128×128 icon; use this, do not upload the `.crx` icon. |
| Small promotional image | `extension/store-assets/promo-440x280.png` | 440×280 PNG, no text, ready for upload. |
| Marquee promotional image | `extension/store-assets/marquee-1400x560.png` | 1400×560 PNG, no text, ready for upload. |
| Screenshot 1 | Create from the real extension popup | 1280×800 or 640×400; show connection status and current-page actions, with personal data hidden. |

Do not use generated imagery as a screenshot. Chrome Web Store screenshots must show the actual extension experience.

## Privacy tab: paste-ready answers

### Single purpose

```text
Enable the locally installed OpenCLI command-line tool to run user-initiated website data commands using the browser session selected by the user.
```

### Data handling disclosure

Declare the categories that the extension can handle: **authentication information** (site cookies), **website content**, **web browsing activity** (URLs and tab metadata), and **user-generated content** when present on a requested page. State that processing occurs locally between the extension and the daemon at `localhost`, that data is not sold or used for advertising, and that no developer-operated server receives browser data.

The Store form must match the actual code and [PRIVACY.md](../PRIVACY.md). Do not mark "no data collected" merely because data stays on the device: Chrome's policy treats local handling as data handling.

## Review notes

```text
OpenCLI is a local companion extension for the OpenCLI command-line tool. To test it, install and start the OpenCLI daemon locally, then use the extension popup to confirm its connection status. The extension has no developer-operated web service and does not require test credentials. Commands are initiated locally by the user and may operate only on websites the user has chosen.
```

## Pre-submission checklist

- [ ] `PRIVACY.md` is committed and publicly reachable through the privacy-policy URL.
- [ ] Upload `opencli-extension-<version>-store.zip`, never a `.crx` or `.pem`.
- [ ] Upload `promo-440x280.png` and one real, redacted extension screenshot.
- [ ] Confirm the Store privacy form declares all data categories above.
- [ ] Confirm the displayed homepage and support URL use `forechoandlook/opencli_rs_plus`.
- [ ] Test the uploaded build with a local daemon and a user-initiated command.
