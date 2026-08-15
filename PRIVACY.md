# OpenCLI Browser Extension Privacy Policy

Last updated: 2026-08-15

OpenCLI is a browser extension that connects a user's locally installed OpenCLI command-line tool to the browser session the user chooses. Its single purpose is to let a user run OpenCLI's user-initiated website data commands with that browser session.

## Data the extension handles

To provide that functionality, the extension may handle website URLs, page content, browser tab metadata, files selected for an OpenCLI command, and authentication cookies for the site involved in that command. Website content can include user-generated content. Authentication cookies are used only to make an authorized request to the relevant website in the user's existing signed-in session.

## How data is used and shared

The extension exchanges command messages only with the OpenCLI daemon running on the same computer at `127.0.0.1` / `localhost`. OpenCLI does not operate a cloud relay, analytics service, or advertising service for this extension. The extension does not sell, rent, or share browser data with third parties.

When the user runs a command, the browser may communicate directly with the website selected by that command, using the user's existing signed-in session. The extension does not send browser data to the OpenCLI developer.

## Storage and retention

The extension stores its local daemon-port preference and short-lived browser task state in Chrome extension storage on the user's device. Files explicitly downloaded by the user are saved through Chrome's download mechanism. Users can remove extension storage by removing the extension and can remove downloaded files through their browser or operating system.

## Permissions

OpenCLI requests browser permissions only to provide its local browser-to-CLI bridge:

- `debugger`, `tabs`, `activeTab`, and host access let OpenCLI run a user-initiated command in the selected website tab.
- `cookies` lets a user-initiated request use the selected website's existing authenticated session.
- `downloads` saves output only when an OpenCLI command requests a download.
- `alarms` and `storage` maintain the local daemon connection and its local configuration.

## Contact

For questions, support, or deletion requests, open an issue at <https://github.com/forechoandlook/opencli_rs_plus/issues>.
