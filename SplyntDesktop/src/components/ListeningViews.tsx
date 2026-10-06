import { Clock3, Flame, Music2, Trophy } from "lucide-react";
import { useMemo, useState } from "react";
import { buildStats, periodTitle, sourceTitle, type ListeningStats, type Ranked, type StatsInput, type StatsPeriod } from "../lib/listeningStats";
import type { LoggedPlay } from "../lib/playLog";
import type { SongSummary } from "../types";
import { TrackTable } from "./Catalog";
import { EmptyState, LoadingState, PageHeading, type SongListHandlers } from "./DesktopViews";
import { MediaArtwork } from "./MediaArtwork";

function dayTitle(at: number, now = new Date()) {
  const date = new Date(at);
  const start = (value: Date) => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();
  const days = Math.round((start(now) - start(date)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return date.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
}

/// Songs played on this computer over the last 90 days, newest first and
/// grouped by day, as on iOS.
export function HistoryView({ onPlay, plays, songList }: { onPlay: (songs: SongSummary[], index: number, label: string) => void; plays: LoggedPlay[]; songList: SongListHandlers }) {
  const days = useMemo(() => {
    const groups: { title: string; songs: SongSummary[] }[] = [];
    for (const play of [...plays].reverse()) {
      const title = dayTitle(play.at);
      const last = groups[groups.length - 1];
      if (last?.title === title) last.songs.push(play.song);
      else groups.push({ title, songs: [play.song] });
    }
    return groups;
  }, [plays]);
  return (
    <>
      <PageHeading title="Listening History" subtitle="Songs you played on this computer in the last 90 days." />
      {days.length === 0 && <EmptyState title="Nothing played yet" body="Songs you play on this computer show up here for 90 days." />}
      {days.map((day) => (
        <section className="result-section history-day" key={day.title}>
          <h2>{day.title}</h2>
          <TrackTable
            currentId={songList.currentId}
            isLiked={songList.isLiked}
            isPlaying={songList.isPlaying}
            onMenu={songList.onMenu}
            onPlay={(index) => {
              // A song played twice that day is queued once.
              const seen = new Set<string>();
              const unique = day.songs.filter((song) => !seen.has(song.id) && Boolean(seen.add(song.id)));
              onPlay(unique, Math.max(0, unique.findIndex((song) => song.id === day.songs[index].id)), "Listening History");
            }}
            onSelect={songList.onSelect}
            onToggleStar={songList.onToggleStar}
            selectedIds={songList.selectedIds}
            showQuality={songList.showQuality}
            songs={day.songs}
          />
        </section>
      ))}
    </>
  );
}

function formatListened(seconds: number) {
  const minutes = Math.round(seconds / 60);
  if (minutes < 600) return { value: minutes.toLocaleString(), unit: "minutes" };
  return { value: Math.round(minutes / 60).toLocaleString(), unit: "hours" };
}

function longDay(day: string) {
  return new Date(`${day}T12:00:00`).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });
}

/// Single-series bars. The title names the series, so there is no legend;
/// each bar carries its value in the app tooltip and in its label.
function BarChart({ bars, label }: { bars: { label: string; tick?: string; value: number }[]; label: string }) {
  const peak = Math.max(1, ...bars.map((bar) => bar.value));
  return (
    <figure className="stats-chart" aria-label={label}>
      <div className="stats-chart__plot">
        {bars.map((bar) => (
          <span
            aria-label={`${bar.label}: ${bar.value.toLocaleString()} ${bar.value === 1 ? "play" : "plays"}`}
            className="stats-chart__bar"
            data-tooltip={`${bar.label} · ${bar.value.toLocaleString()} ${bar.value === 1 ? "play" : "plays"}`}
            key={bar.label}
            role="img"
          >
            <i style={{ height: `${bar.value ? Math.max(3, (bar.value / peak) * 100) : 0}%` }} />
          </span>
        ))}
      </div>
      <div aria-hidden="true" className="stats-chart__axis">{bars.map((bar) => <span key={bar.label}>{bar.tick ?? ""}</span>)}</div>
    </figure>
  );
}

function RankedList({ items, onOpen, round, title }: { items: Ranked[]; onOpen?: (item: Ranked) => void; round?: boolean; title: string }) {
  if (!items.length) return null;
  return (
    <section className="stats-card">
      <h2>{title}</h2>
      <ol className="stats-ranked">
        {items.map((item, index) => (
          <li key={item.id || item.name}>
            <button disabled={!onOpen || !item.id} onClick={() => onOpen?.(item)} type="button">
              <span className="stats-ranked__place">{index + 1}</span>
              <MediaArtwork alt="" className="stats-ranked__art" coverArt={item.coverArt} fallback={round ? "artist" : "album"} shape={round ? "circle" : "square"} />
              <span className="stats-ranked__text"><strong>{item.name}</strong>{item.detail && <small>{item.detail}</small>}</span>
              <span className="stats-ranked__plays">{item.plays.toLocaleString()}</span>
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}

const hourTicks: Record<number, string> = { 0: "12a", 6: "6a", 12: "12p", 18: "6p" };
const weekdayNames = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

function Highlights({ stats }: { stats: ListeningStats }) {
  const items = [
    stats.peakDay && { icon: Trophy, title: "Biggest day", value: `${stats.peakDay.plays.toLocaleString()} plays`, detail: longDay(stats.peakDay.day) },
    stats.longestStreak && { icon: Flame, title: "Longest streak", value: `${stats.longestStreak.days} ${stats.longestStreak.days === 1 ? "day" : "days"}`, detail: `Ended ${longDay(stats.longestStreak.endedOn)}` },
    { icon: Clock3, title: "Current streak", value: stats.currentStreak ? `${stats.currentStreak.days} ${stats.currentStreak.days === 1 ? "day" : "days"}` : "None", detail: stats.currentStreak ? "Keep it going today" : "Play something to start one" },
  ].filter(Boolean) as { icon: typeof Trophy; title: string; value: string; detail: string }[];
  return (
    <div className="stats-highlights">
      {items.map(({ icon: Icon, title, value, detail }) => (
        <div key={title}><Icon aria-hidden="true" size={18} /><span><small>{title}</small><strong>{value}</strong><small>{detail}</small></span></div>
      ))}
    </div>
  );
}

/// Listening stats from the best source the server offers, laid out as the
/// iOS Stats page's core: totals, top lists, and when you listen.
export function StatsView({ input, loading, onOpenAlbum, onOpenArtist, onPlaySong }: {
  input?: StatsInput;
  loading: boolean;
  onOpenAlbum: (id: string) => void;
  onOpenArtist: (id: string) => void;
  onPlaySong: (song: SongSummary) => void;
}) {
  const [period, setPeriod] = useState<StatsPeriod>("allTime");
  const stats = useMemo(() => input ? buildStats(input, period) : undefined, [input, period]);
  if (loading && !stats) return <LoadingState label="Reading your listening history" />;
  if (!stats) return null;
  const listened = formatListened(stats.secondsListened);
  return (
    <>
      <PageHeading title="Stats" subtitle={sourceTitle[stats.source]} />
      {stats.hasTimeline && (
        <div aria-label="Period" className="stats-periods" role="radiogroup">
          {(Object.keys(periodTitle) as StatsPeriod[]).map((option) => (
            <button aria-checked={period === option} className={period === option ? "filter-chip filter-chip--active" : "filter-chip"} key={option} onClick={() => setPeriod(option)} role="radio" type="button">{periodTitle[option]}</button>
          ))}
        </div>
      )}
      {stats.plays === 0 ? <EmptyState title="No plays in this period" body="Play some music and it will show up here." /> : (
        <>
          <div className="stats-totals">
            <div className="stats-totals__hero"><strong>{listened.value}</strong><small>{listened.unit} listened</small></div>
            <div><strong>{stats.plays.toLocaleString()}</strong><small>plays</small></div>
            <div><strong>{stats.distinctTracks.toLocaleString()}</strong><small>songs</small></div>
            <div><strong>{stats.distinctArtists.toLocaleString()}</strong><small>artists</small></div>
            <div><strong>{stats.distinctAlbums.toLocaleString()}</strong><small>albums</small></div>
          </div>
          {stats.hasTimeline && <Highlights stats={stats} />}
          <div className="stats-grid">
            <RankedList items={stats.topTracks} onOpen={(item) => item.song && onPlaySong(item.song)} title="Top songs" />
            <RankedList items={stats.topArtists} onOpen={(item) => onOpenArtist(item.id)} round title="Top artists" />
            <RankedList items={stats.topAlbums} onOpen={(item) => onOpenAlbum(item.id)} title="Top albums" />
            {stats.topGenres.length > 0 && (
              <section className="stats-card">
                <h2>Top genres</h2>
                <ol className="stats-genres">{stats.topGenres.map((genre) => <li key={genre.name}><Music2 aria-hidden="true" size={15} /><strong>{genre.name}</strong><span>{genre.plays.toLocaleString()}</span></li>)}</ol>
              </section>
            )}
          </div>
          {stats.hasTimeline && (
            <div className="stats-grid">
              <section className="stats-card">
                <h2>Time of day</h2>
                <BarChart bars={stats.hours.map((value, hour) => ({ label: new Date(2000, 0, 1, hour).toLocaleTimeString(undefined, { hour: "numeric" }), tick: hourTicks[hour], value }))} label="Plays by hour of the day" />
              </section>
              <section className="stats-card">
                <h2>Day of the week</h2>
                <BarChart bars={stats.weekdays.map((value, day) => ({ label: weekdayNames[day], tick: weekdayNames[day].slice(0, 3), value }))} label="Plays by day of the week" />
              </section>
            </div>
          )}
          {stats.unresolvedPlays > 0 && <p className="stats-footnote">{stats.unresolvedPlays.toLocaleString()} {stats.unresolvedPlays === 1 ? "play is" : "plays are"} left out because the song is no longer on your server.</p>}
        </>
      )}
    </>
  );
}
