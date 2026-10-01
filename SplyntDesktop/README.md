# Splynt Desktop

The Splynt desktop client for Windows, Linux and macOS, built with Tauri 2,
React and Rust. For what Splynt is and where else it runs, see the
[main README](../README.md).

## Download

Installers for Windows and Linux are on the
[latest release](https://github.com/elijahgronda/Splynt/releases/latest).

| Platform | File |
| --- | --- |
| Windows | `.exe` (NSIS) or `.msi` |
| Linux | `.AppImage` or `.deb` |

The builds are not signed, so Windows SmartScreen may stop the installer the
first time. Choose **More info**, then **Run anyway**.

There is no macOS installer. Gatekeeper blocks an unsigned Mac app, and
signing one for distribution needs a paid Apple Developer ID. Mac users can
build it from source, below.

## Development

You need Node.js 20 or newer, stable Rust, and the platform prerequisites
listed by [Tauri](https://tauri.app/start/prerequisites/).

```sh
cd SplyntDesktop
npm install
npm run test
npm run tauri dev
```

`npm run tauri dev` opens the app against the real Rust host, so playback,
downloads and Splynt Connect behave the way they will in a release build.
Changes under `src/` reload in place. Changes under `src-tauri/` rebuild the
host and relaunch the window.

### Building on a Mac

```sh
npm run tauri:build:mac
```

This builds a `.app` and a `.dmg`, signed with the first Developer ID or Apple
Development certificate in your keychain. If you have neither, open Xcode,
sign in with your Apple ID under Settings, Accounts, and let it create a free
Apple Development certificate. A build signed that way runs on your own Mac.

### Release builds

Installers have to be built on the system they target. The
[Desktop installers](../.github/workflows/desktop-installers.yml) workflow
builds Windows and Linux on GitHub Actions. Pushing a `v*` tag runs it and
attaches the installers to a draft release. Running it by hand from the Actions
tab attaches them to the run instead, and publishes nothing.

## License

[GNU GPL v3.0 or later](../LICENSE).
