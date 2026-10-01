# Splynt Desktop

This is the Splynt desktop app for Windows, Linux and macOS. It's made with
Tauri 2, React and Rust. For everything else about Splynt, check out the
[main README](../README.md).

## Download

Windows and Linux installers are on the
[latest release](https://github.com/elijahgronda/Splynt/releases/latest).
macOS test builds are attached to manual
[Desktop installers runs](https://github.com/elijahgronda/Splynt/actions/workflows/desktop-installers.yml).

| Platform | File |
| --- | --- |
| Windows | `.exe` (NSIS) or `.msi` |
| Linux | `.AppImage` or `.deb` |
| macOS test | `.dmg` for Apple Silicon or Intel |

The builds aren't signed yet, so Windows SmartScreen might stop the installer
the first time. Hit **More info**, then **Run anyway**.

The macOS test builds use an ad-hoc signature. They are not notarized or
published in releases. After copying Splynt from the `.dmg`, try to open it,
then use **System Settings > Privacy & Security > Open Anyway** if Gatekeeper
blocks it. A regular Mac release still needs Developer ID signing and
notarization. You can also build the app on your own Mac below.

## Running it yourself

You'll need Node.js 20 or newer, Rust (stable), and whatever
[Tauri](https://tauri.app/start/prerequisites/) needs for your system.

```sh
cd SplyntDesktop
npm install
npm run test
npm run tauri dev
```

`npm run tauri dev` opens the real app, so playback, downloads and Splynt
Connect all work the same as a release build. Changes in `src/` reload right
away. Changes in `src-tauri/` rebuild the Rust side and reopen the window,
which takes a bit longer.

### Building on a Mac

```sh
npm run tauri:build:mac
```

This makes a `.app` and a `.dmg`, signed with the first Developer ID or Apple
Development certificate in your keychain. If you don't have either one, open
Xcode, sign in with your Apple ID under Settings, Accounts, and let it make a
free Apple Development certificate. A build signed that way runs on your own
Mac.

### Release builds

Installers have to be built on the system they're for, so the
[Desktop installers](../.github/workflows/desktop-installers.yml) workflow
builds Windows and Linux on GitHub Actions. Pushing a `v*` tag runs it and
puts those installers on a draft release. A manual run can also build macOS
test artifacts for both chip types. It attaches installers to the run without
publishing a release.

## License

[GPL-3.0 or later](../LICENSE).
