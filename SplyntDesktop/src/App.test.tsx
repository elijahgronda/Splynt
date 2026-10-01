import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("Splynt Desktop authentication", () => {
  beforeEach(() => {
    localStorage.clear();
    invoke.mockReset();
    invoke.mockImplementation((command: string) => command === "restore_session" ? Promise.resolve(null) : Promise.resolve(undefined));
  });

  it("opens on login with no sample library", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Connect to your server" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Main navigation" })).not.toBeInTheDocument();
    expect(screen.queryByText("No track selected")).not.toBeInTheDocument();
  });

  it("lets people skip a stalled automatic sign-in", async () => {
    vi.useFakeTimers();
    invoke.mockImplementation((command: string) => command === "restore_session" ? new Promise(() => undefined) : Promise.resolve(undefined));
    const view = render(<App />);

    expect(screen.getByRole("main", { name: "Opening Splynt" })).toBeInTheDocument();
    await act(async () => {
      vi.advanceTimersByTime(3_500);
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue without waiting" }));

    expect(screen.getByRole("heading", { name: "Connect to your server" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Automatic sign-in was skipped");
    view.unmount();
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("enters the real server shell only after a successful login", async () => {
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [{ id: "server-album-1", title: "Server Album", artist: "Server Artist", year: 2026 }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.resolve(null);
      if (command === "connect_server") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: connected.albums, recent: [], frequent: [], random: [] });
      if (command === "load_library") return Promise.resolve({ albums: connected.albums, artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      if (command === "media_url") return Promise.resolve("splice-media://localhost/cover?id=cover");
      return Promise.resolve(undefined);
    });
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Server address"), { target: { value: "https://music.example.test" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "elijah" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() => expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeInTheDocument());
    expect(screen.getByText("Server Album")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("connect_server", {
      request: {
        server: "https://music.example.test",
        username: "elijah",
        password: "secret",
        allowsSelfSigned: false,
        rememberMe: true,
      },
    });
  });

  it("opens the library rail at its default width on a fresh profile", async () => {
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah" },
      albums: [{ id: "server-album-1", title: "Server Album", artist: "Server Artist" }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.resolve(null);
      if (command === "connect_server") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: connected.albums, recent: [], frequent: [], random: [], genres: [] });
      if (command === "load_library") return Promise.resolve({ albums: connected.albums, artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      return Promise.resolve(undefined);
    });
    render(<App />);
    fireEvent.change(await screen.findByLabelText("Server address"), { target: { value: "https://music.example.test" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "elijah" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    // A missing width used to read back as 0 and collapse the rail to its 72px
    // minimum, hiding library search, filters and collection names.
    const nav = await screen.findByRole("navigation", { name: "Main navigation" });
    const shell = nav.closest(".desktop-shell") as HTMLElement;
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("280px");
    expect(shell.style.getPropertyValue("--context-width")).toBe("350px");
    expect(screen.getByLabelText("Search your library")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open account menu" }));
    expect(screen.getByRole("menuitem", { name: /Sign out/ })).toBeInTheDocument();
  });

  it("separates switching accounts from removing a saved login", async () => {
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: [], recent: [], frequent: [], random: [] });
      if (command === "load_library") return Promise.resolve({ albums: [], artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      if (command === "connect_snapshot") return new Promise(() => undefined);
      return Promise.resolve(undefined);
    });
    render(<App />);

    await screen.findByRole("navigation", { name: "Main navigation" });
    fireEvent.click(screen.getByRole("button", { name: "Open account menu" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Switch account" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("disconnect_server", { forgetSavedLogin: false }));
    expect(screen.getByRole("heading", { name: "Connect to your server" })).toBeInTheDocument();
  });

  it("asks before signing out and removes the saved login", async () => {
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: [], recent: [], frequent: [], random: [] });
      if (command === "load_library") return Promise.resolve({ albums: [], artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      if (command === "connect_snapshot") return new Promise(() => undefined);
      return Promise.resolve(undefined);
    });
    render(<App />);

    await screen.findByRole("navigation", { name: "Main navigation" });
    fireEvent.click(screen.getByRole("button", { name: "Open account menu" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    const dialog = screen.getByRole("dialog", { name: "Sign out elijah?" });
    expect(dialog).toHaveTextContent("removes the saved login");
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign out" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("disconnect_server", { forgetSavedLogin: true }));
    expect(screen.getByRole("heading", { name: "Connect to your server" })).toBeInTheDocument();
  });

  it("keeps login visible after a rejected connection", async () => {
    invoke.mockImplementation((command: string) => command === "restore_session" ? Promise.resolve(null) : Promise.reject("Wrong username or password."));
    render(<App />);
    fireEvent.change(await screen.findByLabelText("Server address"), { target: { value: "https://music.example.test" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "elijah" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Wrong username or password.");
    expect(screen.getByRole("heading", { name: "Connect to your server" })).toBeInTheDocument();
  });

  it("opens the last cached library when a saved server is temporarily offline", async () => {
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [{ id: "cached-album", title: "Cached Album", artist: "Cached Artist" }],
      connection: { status: "online" as const },
    };
    localStorage.setItem("splice.desktop.active-cache.v1", JSON.stringify({
      identity: "music.example.test|elijah",
      library: connected,
      savedAt: Date.now(),
    }));
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.reject("Could not reach music.example.test.");
      if (command === "load_home" || command === "load_library") return Promise.reject("Offline");
      if (command === "connect_snapshot") return Promise.resolve({ isAvailable: false, peers: [], commands: [] });
      return Promise.resolve(undefined);
    });

    render(<App />);

    expect(await screen.findByRole("navigation", { name: "Main navigation" })).toBeInTheDocument();
    expect(screen.getByText("Server unavailable")).toBeInTheDocument();
    expect(screen.getByText("Cached Album")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Connect to your server" })).not.toBeInTheDocument();
  });

  it("drives another device from the player bar once playback moves there", async () => {
    const song = { id: "server-song-1", title: "Server Song", artist: "Server Artist", album: "Server Album", duration: 200, coverArt: "cover" };
    const album = { id: "server-album-1", title: "Server Album", artist: "Server Artist", year: 2026 };
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [album],
    };
    const peer = {
      id: "peer-tv", name: "Living Room", platform: "tvOS",
      playback: { trackID: "server-song-1", title: "Server Song", artist: "Server Artist", album: "Server Album", coverArtID: "cover", isPlaying: true, position: 12, duration: 200 },
      updatedAt: 0,
    };
    const sent: { peerId: string; command: { name: string; value?: number } }[] = [];
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "restore_session") return Promise.resolve(null);
      if (command === "connect_server") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: [album], recent: [], frequent: [], random: [] });
      if (command === "load_library") return Promise.resolve({ albums: [album], artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      if (command === "get_album") return Promise.resolve({ ...album, songs: [song], duration: song.duration });
      if (command === "media_url") return Promise.resolve("splice-media://localhost/media?id=server-song-1");
      if (command === "connect_snapshot") return Promise.resolve({ isAvailable: true, localDeviceId: "this-desktop", peers: [peer], commands: [] });
      if (command === "send_connect_command") {
        sent.push(args as { peerId: string; command: { name: string } });
        return Promise.resolve(undefined);
      }
      return Promise.resolve(undefined);
    });
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Server address"), { target: { value: "https://music.example.test" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "elijah" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByRole("navigation", { name: "Main navigation" });

    fireEvent.click(await screen.findByRole("button", { name: "Play Server Album" }));
    expect(await screen.findByText("Server Song")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Splynt Connect devices" }));
    const devices = await screen.findByRole("complementary", { name: "Devices" });
    fireEvent.click(await within(devices).findByRole("button", { name: "Play on Living Room" }));

    // The bar now says where the audio is, and its transport addresses that peer.
    const bar = screen.getByLabelText("Player");
    await within(bar).findByRole("button", { name: /Playing on Living Room/ });
    expect(sent.map((entry) => entry.command.name)).toContain("handoff");

    sent.length = 0;
    fireEvent.click(within(bar).getByRole("button", { name: "Next" }));
    fireEvent.click(within(bar).getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(sent.map((entry) => entry.command.name)).toEqual(["next", "previous"]));
    expect(sent.every((entry) => entry.peerId === "peer-tv")).toBe(true);

    // Play state is the peer's, not this device's silent audio element.
    expect(within(bar).getByRole("button", { name: "Pause" })).toBeInTheDocument();

    // Taking it back ends the mode. The button says which computer "here" is:
    // it used to sit beside "Play on Living Room" at equal weight, with the
    // two words pointing at different machines.
    fireEvent.click(within(devices).getByRole("button", { name: "Play on this computer" }));
    await waitFor(() => expect(within(bar).queryByRole("button", { name: /Playing on Living Room/ })).not.toBeInTheDocument());
  });

  it("keeps the persistent player mounted across panels and expanded playback", async () => {
    const album = { id: "server-album-1", title: "Server Album", artist: "Server Artist", year: 2026 };
    const song = { id: "server-song-1", title: "Server Song", artist: "Server Artist", album: album.title, albumId: album.id, duration: 225 };
    const connected = {
      server: { displayHost: "music.example.test", username: "elijah", serverType: "navidrome" },
      albums: [album],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "restore_session") return Promise.resolve(null);
      if (command === "connect_server") return Promise.resolve(connected);
      if (command === "load_home") return Promise.resolve({ newest: [album], recent: [], frequent: [], random: [] });
      if (command === "load_library") return Promise.resolve({ albums: [album], artists: [], playlists: [], starredSongs: [], starredAlbums: [], starredArtists: [] });
      if (command === "get_album") return Promise.resolve({ ...album, songs: [song], duration: song.duration });
      if (command === "connect_snapshot") return new Promise(() => undefined);
      if (command === "media_url") return Promise.resolve("splice-media://localhost/media?id=server-song-1");
      if (command === "get_lyrics") return Promise.resolve({ synced: true, lines: [{ start: 0, value: "First lyric" }, { start: 12, value: "Second lyric" }] });
      return Promise.resolve(undefined);
    });
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Server address"), { target: { value: "https://music.example.test" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "elijah" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await screen.findByRole("navigation", { name: "Main navigation" });

    fireEvent.click(await screen.findByRole("button", { name: "Play Server Album" }));
    expect(await screen.findByText("Server Song")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Queue" }));
    expect(screen.getByRole("complementary", { name: "Queue" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close Queue" }));
    expect(screen.queryByRole("complementary", { name: "Queue" })).not.toBeInTheDocument();
    const artworkToggle = screen.getAllByRole("button", { name: "Now playing view" })[0];
    fireEvent.click(artworkToggle);
    expect(screen.getByRole("complementary", { name: "Now Playing" })).toBeInTheDocument();
    expect(localStorage.getItem("splice.panel")).toBe("nowPlaying");
    fireEvent.click(artworkToggle);
    expect(screen.queryByRole("complementary", { name: "Now Playing" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open full player" }));
    const fullPlayer = screen.getByRole("region", { name: "Expanded player" });
    expect(fullPlayer).toBeInTheDocument();
    expect(screen.getByLabelText("Player")).toBeInTheDocument();
    expect(within(fullPlayer).queryByRole("button", { name: "Shuffle" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Shuffle" })).toBeInTheDocument();
    expect(within(fullPlayer).getByRole("button", { name: "Exit full player" })).toHaveFocus();
    fireEvent.click(within(fullPlayer).getByRole("button", { name: "More options" }));
    fireEvent.click(within(fullPlayer).getByRole("menuitem", { name: "Open queue" }));
    expect(screen.getByRole("complementary", { name: "Queue" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Expanded player" })).toBeInTheDocument();
    fireEvent.click(within(fullPlayer).getByRole("radio", { name: "Show lyrics" }));
    expect(within(fullPlayer).getByRole("region", { name: "Lyrics" })).toHaveTextContent("First lyric");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "Expanded player" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Player")).toBeInTheDocument();

    // The player bar's mic opens the now-playing view with lyrics beside the
    // art, and a second press slides them back out.
    const bar = screen.getByLabelText("Player");
    fireEvent.click(within(bar).getByRole("button", { name: "Lyrics" }));
    const lyricsView = screen.getByRole("region", { name: "Expanded player" });
    expect(within(lyricsView).getByRole("radio", { name: "Show lyrics" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(within(bar).getByRole("button", { name: "Lyrics" }));
    expect(within(lyricsView).getByRole("radio", { name: "Show artwork" })).toHaveAttribute("aria-checked", "true");
  });
});
