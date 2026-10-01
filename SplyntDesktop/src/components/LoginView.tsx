import { invoke } from "@tauri-apps/api/core";
import { Eye, EyeOff, Globe2, LockKeyhole, Server, Trash2, UserRound } from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import { useDialogFocus } from "../hooks/useDialogFocus";
import type { ConnectedLibrary, ConnectRequest, SavedProfileSummary } from "../types";
import { Brand } from "./Brand";

type LoginViewProps = {
  initialError?: string;
  onConnected: (library: ConnectedLibrary) => void;
};

export function LoginView({ initialError, onConnected }: LoginViewProps) {
  const [server, setServer] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [rememberMe, setRememberMe] = useState(true);
  const [allowsSelfSigned, setAllowsSelfSigned] = useState(false);
  const [showsPassword, setShowsPassword] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectingProfile, setConnectingProfile] = useState<string>();
  const [profiles, setProfiles] = useState<SavedProfileSummary[]>([]);
  const [profileToForget, setProfileToForget] = useState<SavedProfileSummary>();
  const [error, setError] = useState<string | undefined>(initialError);

  const canConnect = server.trim() && username.trim() && password;

  useEffect(() => {
    let active = true;
    invoke<SavedProfileSummary[]>("list_profiles")
      .then((saved) => active && setProfiles(Array.isArray(saved) ? saved : []))
      .catch(() => undefined);
    return () => { active = false };
  }, []);

  async function connectSaved(profile: SavedProfileSummary) {
    if (isConnecting || connectingProfile) return;
    setError(undefined);
    setConnectingProfile(profile.id);
    try {
      onConnected(await invoke<ConnectedLibrary>("connect_profile", { profileId: profile.id }));
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Splynt could not connect to that saved server.");
    } finally {
      setConnectingProfile(undefined);
    }
  }

  async function forgetSaved(profile: SavedProfileSummary) {
    try {
      await invoke("forget_profile", { profileId: profile.id });
      setProfiles((items) => items.filter((item) => item.id !== profile.id));
      setProfileToForget(undefined);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "That saved server could not be forgotten.");
    }
  }

  async function connect(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canConnect || isConnecting) return;

    setError(undefined);
    setIsConnecting(true);
    const request: ConnectRequest = {
      server: server.trim(),
      username: username.trim(),
      password,
      allowsSelfSigned,
      rememberMe,
    };

    try {
      const library = await invoke<ConnectedLibrary>("connect_server", { request });
      onConnected(library);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Splynt could not connect to that server.");
    } finally {
      setIsConnecting(false);
    }
  }

  return (
    <main className="login-page">
      <div className="login-page__glow" aria-hidden="true" />
      <section className="login-card" aria-labelledby="login-title">
        <Brand />
        <div className="login-card__intro">
          <p className="eyebrow">YOUR MUSIC, EVERYWHERE</p>
          <h1 id="login-title">Connect to your server</h1>
          <p>Sign in to your Navidrome or Subsonic-compatible library.</p>
        </div>

        {profiles.length > 0 && (
          <section className="saved-profiles" aria-label="Saved servers">
            <h2>Saved servers</h2>
            {profiles.map((profile) => (
              <div className="saved-profile" key={profile.id}>
                <button className="saved-profile__connect" disabled={Boolean(connectingProfile) || isConnecting} onClick={() => void connectSaved(profile)} type="button">
                  <span><strong>{profile.displayHost}</strong><small>{profile.username}{profile.allowsSelfSigned ? " · Self-signed TLS" : ""}</small></span>
                  {connectingProfile === profile.id ? <span className="spinner" aria-hidden="true" /> : <Server size={17} />}
                </button>
                <button aria-label={`Forget ${profile.displayHost}`} className="saved-profile__forget" onClick={() => setProfileToForget(profile)} title="Forget saved server" type="button"><Trash2 size={15} /></button>
              </div>
            ))}
            <div className="login-divider"><span>or connect another server</span></div>
          </section>
        )}

        <form onSubmit={connect}>
          <label className="login-field">
            <span>Server address</span>
            <span className="login-field__control">
              <Globe2 size={18} aria-hidden="true" />
              <input
                autoCapitalize="none"
                autoCorrect="off"
                autoFocus
                onChange={(event) => setServer(event.target.value)}
                placeholder="https://music.home.lan:4533"
                spellCheck={false}
                inputMode="url"
                type="text"
                value={server}
              />
            </span>
          </label>

          <label className="login-field">
            <span>Username</span>
            <span className="login-field__control">
              <UserRound size={18} aria-hidden="true" />
              <input
                autoCapitalize="none"
                autoComplete="username"
                onChange={(event) => setUsername(event.target.value)}
                placeholder="username"
                spellCheck={false}
                value={username}
              />
            </span>
          </label>

          <label className="login-field">
            <span>Password</span>
            <span className="login-field__control">
              <LockKeyhole size={18} aria-hidden="true" />
              <input
                autoComplete="current-password"
                onChange={(event) => setPassword(event.target.value)}
                placeholder="Password"
                type={showsPassword ? "text" : "password"}
                value={password}
              />
              <button
                aria-label={showsPassword ? "Hide password" : "Show password"}
                className="field-icon-button"
                onClick={() => setShowsPassword((visible) => !visible)}
                type="button"
              >
                {showsPassword ? <EyeOff size={18} /> : <Eye size={18} />}
              </button>
            </span>
          </label>

          <label className="check-row">
            <input
              checked={rememberMe}
              onChange={(event) => setRememberMe(event.target.checked)}
              type="checkbox"
            />
            <span>
              Keep me signed in
              <small>Your login is stored in this computer&apos;s secure credential vault.</small>
            </span>
          </label>

          <label className="check-row">
            <input
              checked={allowsSelfSigned}
              onChange={(event) => setAllowsSelfSigned(event.target.checked)}
              type="checkbox"
            />
            <span>
              Allow self-signed certificate
              <small>Use only for a server you trust on your local network.</small>
            </span>
          </label>

          {error && <div className="login-error" role="alert">{error}</div>}

          <button className="login-submit" disabled={!canConnect || isConnecting || Boolean(connectingProfile)} type="submit">
            {isConnecting ? <span className="spinner" aria-hidden="true" /> : <Server size={18} aria-hidden="true" />}
            {isConnecting ? "Connecting…" : "Connect"}
          </button>
        </form>

        <p className="privacy-note"><LockKeyhole size={13} /> Credentials stay on this device.</p>
      </section>
      {profileToForget && (
        <ForgetProfileDialog
          onCancel={() => setProfileToForget(undefined)}
          onConfirm={() => void forgetSaved(profileToForget)}
          profile={profileToForget}
        />
      )}
    </main>
  );
}

function ForgetProfileDialog({ onCancel, onConfirm, profile }: { onCancel: () => void; onConfirm: () => void; profile: SavedProfileSummary }) {
  const dialogRef = useDialogFocus<HTMLElement>(onCancel);
  return (
    <div className="modal-scrim">
      <section aria-labelledby="forget-profile-title" aria-modal="true" className="desktop-modal" ref={dialogRef} role="dialog">
        <p className="eyebrow">SAVED SERVER</p>
        <h2 id="forget-profile-title">Forget {profile.displayHost}?</h2>
        <p className="modal-copy">This removes the saved username and password from this computer. It does not change the server or delete cached music.</p>
        <div>
          <button onClick={onCancel} type="button">Cancel</button>
          <button className="modal-danger" onClick={onConfirm} type="button">Forget server</button>
        </div>
      </section>
    </div>
  );
}
