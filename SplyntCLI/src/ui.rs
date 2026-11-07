//! Drawing. Every frame rebuilds `app.hits`, the clickable regions, so the
//! mouse always matches what is on screen.

use crate::api::{Lyrics, Song};
use crate::app::prefs::{SPage, SRow};
use crate::app::*;
use crate::art::Art;
use crate::player::BANDS;
use crate::settings::Visualizer;
use crate::theme::{self, *};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear};
use ratatui_image::Image;
use std::collections::HashSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

fn put(buf: &mut Buffer, x: u16, y: u16, w: u16, s: &str, style: Style) -> u16 {
    if w == 0 || y >= buf.area.bottom() || x >= buf.area.right() {
        return 0;
    }
    let w = w.min(buf.area.right() - x);
    let t = fit(s, w as usize);
    buf.set_string(x, y, &t, style);
    t.width() as u16
}

fn put_right(buf: &mut Buffer, right: u16, y: u16, w: u16, s: &str, style: Style) -> u16 {
    let t = fit(s, w as usize);
    let tw = t.width() as u16;
    put(buf, right.saturating_sub(tw), y, tw, &t, style)
}

fn fill(buf: &mut Buffer, r: Rect, c: Color) {
    let r = r.intersection(buf.area);
    buf.set_style(r, Style::default().bg(c));
}

fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
    Rect { x, y, width: w, height: h }
}

/// Wraps text to `w` columns.
fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
            cur = word.to_string();
        } else if cur.width() + 1 + word.width() <= w {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

struct Ctx<'a> {
    art: &'a mut Option<Art>,
    hits: &'a mut Vec<(Rect, Hit)>,
    downloaded: &'a HashSet<String>,
    playing: Option<String>,
    paused: bool,
}

impl Ctx<'_> {
    fn hit(&mut self, r: Rect, h: Hit) {
        self.hits.push((r, h));
    }
}

fn accent_rgb() -> (u8, u8, u8) {
    match ACCENT() {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (43, 224, 107),
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    app.slider_rects.clear();
    let area = f.area();
    f.buffer_mut().set_style(area, Style::default().bg(BG).fg(INK));
    if matches!(app.screen, Screen::Login(_)) {
        draw_login(f, app, area);
        return;
    }
    let [body, bar, hints] =
        Layout::vertical([Constraint::Min(6), Constraint::Length(5), Constraint::Length(1)]).areas(area);
    let side_w = if area.width >= 110 { 28 } else if area.width >= 76 { 22 } else { 6 };
    let [side, main] = Layout::horizontal([Constraint::Length(side_w), Constraint::Min(10)]).areas(body);

    draw_sidebar(f, app, side);
    draw_main(f, app, main);
    draw_player(f, app, bar);
    draw_hints(f, app, hints);

    if let Some((msg, _, err)) = &app.toast {
        let w = (msg.width() as u16 + 6).min(main.width.saturating_sub(4));
        let r = rect(main.right().saturating_sub(w + 2), main.y + 1, w, 3);
        f.render_widget(Clear, r);
        let b = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if *err { ERROR } else { ACCENT() }))
            .style(Style::default().bg(SEL));
        f.render_widget(b, r);
        put(f.buffer_mut(), r.x + 2, r.y + 1, w.saturating_sub(4), msg, ink());
    }

    match &app.overlay {
        Some(Overlay::Help) => draw_help(f, app, area),
        Some(Overlay::Menu(_)) => draw_menu(f, app, area),
        Some(Overlay::Prompt(_)) => draw_prompt(f, app, area),
        None => {}
    }
}

// ---------------------------------------------------------------- art

fn placeholder(buf: &mut Buffer, seed: &str, r: Rect, liked: bool) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    let (a, b) = if liked {
        ((0x4E, 0x7F, 0xE0), (0x8C, 0x67, 0xE0))
    } else if seed == "downloads" {
        let a = accent_rgb();
        (a, (a.0 / 4, a.1 / 4, a.2 / 4))
    } else {
        let c = seed_rgb(seed);
        (c, (c.0 / 3, c.1 / 3, c.2 / 3))
    };
    for row in 0..r.height {
        let t = row as f32 / r.height.max(2).saturating_sub(1) as f32;
        fill(buf, rect(r.x, r.y + row, r.width, 1), theme::mix(a, b, t));
    }
    let g = if liked {
        "♥".to_string()
    } else if seed == "downloads" {
        "↓".to_string()
    } else {
        glyph(seed).to_string()
    };
    let ink = if liked { Color::White } else { readable_ink(a) };
    let gx = r.x + r.width / 2;
    let gy = r.y + r.height / 2;
    put(buf, gx, gy, 1, &g, Style::default().fg(ink).add_modifier(Modifier::BOLD));
}

fn draw_art(f: &mut Frame, art: &mut Option<Art>, cover: Option<&str>, seed: &str, r: Rect, liked: bool) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    if let (Some(id), Some(a)) = (cover, art.as_mut()) {
        if let Some(p) = a.get(id, r.width, r.height) {
            let s = p.size();
            let x = r.x + r.width.saturating_sub(s.width) / 2;
            let y = r.y + r.height.saturating_sub(s.height) / 2;
            let target = rect(x, y, s.width.min(r.width), s.height.min(r.height));
            f.render_widget(Image::new(p), target);
            return;
        }
    }
    placeholder(f.buffer_mut(), seed, r, liked);
}

fn art_rows(area: Rect) -> u16 {
    if area.height >= 46 && area.width >= 150 {
        10
    } else if area.height >= 30 {
        8
    } else {
        6
    }
}

// ---------------------------------------------------------------- cards

#[allow(clippy::too_many_arguments)]
fn card(f: &mut Frame, ctx: &mut Ctx, item: &Item, x: u16, y: u16, art_h: u16, selected: bool, focused: bool, hit: Hit) {
    let art_w = art_h * 2;
    let r = rect(x, y, art_w + 2, art_h + 4);
    if selected {
        fill(f.buffer_mut(), r, if focused { SEL } else { CARD });
    }
    let art_r = rect(x + 1, y + 1, art_w, art_h);
    draw_art(f, ctx.art, item.cover(), item.id(), art_r, false);
    let title_style = if selected && focused { bold().fg(ACCENT()) } else { bold() };
    let playing = matches!(item, Item::Song(s) if Some(&s.id) == ctx.playing.as_ref());
    let title = if playing { format!("♪ {}", item.title()) } else { item.title().to_string() };
    put(f.buffer_mut(), x + 1, y + art_h + 1, art_w, &title, title_style);
    put(f.buffer_mut(), x + 1, y + art_h + 2, art_w, &item.subtitle(), muted());
    ctx.hit(r, hit);
}

fn draw_grid(f: &mut Frame, ctx: &mut Ctx, grid: &mut Grid, area: Rect, focused: bool, art_h: u16, hit: fn(usize) -> Hit) {
    let art_w = art_h * 2;
    let cw = art_w + 3;
    let ch = art_h + 4;
    let cols = ((area.width.saturating_sub(1)) / cw).max(1) as usize;
    let rows = (area.height / ch).max(1) as usize;
    grid.cols = cols;
    grid.rows_visible = rows;
    if grid.items.is_empty() {
        return;
    }
    grid.sel = grid.sel.min(grid.items.len() - 1);
    let srow = grid.sel / cols;
    if srow < grid.scroll {
        grid.scroll = srow;
    }
    if srow >= grid.scroll + rows {
        grid.scroll = srow + 1 - rows;
    }
    for r in 0..rows {
        for c in 0..cols {
            let i = (grid.scroll + r) * cols + c;
            let Some(item) = grid.items.get(i) else { break };
            let x = area.x + 1 + c as u16 * cw;
            let y = area.y + r as u16 * ch;
            if y + ch > area.bottom() + 1 {
                continue;
            }
            card(f, ctx, item, x, y, art_h, i == grid.sel, focused, hit(i));
        }
    }
    let total_rows = grid.items.len().div_ceil(cols);
    if total_rows > rows {
        scrollbar(f.buffer_mut(), area, grid.scroll, rows, total_rows);
    }
}

fn scrollbar(buf: &mut Buffer, area: Rect, offset: usize, visible: usize, total: usize) {
    if total <= visible || area.height < 3 {
        return;
    }
    let x = area.right().saturating_sub(1);
    let h = area.height as usize;
    let thumb = ((visible * h) / total).max(1);
    let start = (offset * h) / total;
    for i in 0..h {
        let s = if i >= start && i < start + thumb { "┃" } else { "│" };
        let style = if i >= start && i < start + thumb { muted() } else { Style::default().fg(FAINT) };
        put(buf, x, area.y + i as u16, 1, s, style);
    }
}

// ---------------------------------------------------------------- sidebar

const LOGO: [&str; 2] = ["▄▀▀ █▀▄ █   █ █ █▄ █ ▀█▀", "▄▄▀ █▀  █▄▄  █  █ ▀█  █ "];

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    fill(f.buffer_mut(), area, SIDEBAR);
    let compact = area.width < 12;
    let mut y = area.y + 1;
    if area.width >= 28 {
        for l in LOGO {
            put(f.buffer_mut(), area.x + 2, y, area.width - 2, l, accent());
            y += 1;
        }
    } else if !compact {
        put(f.buffer_mut(), area.x + 2, y, area.width - 2, "◉ splynt", bold().fg(ACCENT()));
        y += 1;
    } else {
        put(f.buffer_mut(), area.x + 2, y, 2, "◉", accent());
        y += 1;
    }
    y += 1;
    let root = app.root_index();
    let downloading = app.dl_progress.lock().map(|p| p.current.is_some()).unwrap_or(false);
    for (i, (icon, label)) in NAV.iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        if i == NAV_DOWNLOADS {
            // A little air before the second group.
            y += 1;
        }
        let row = rect(area.x, y, area.width, 1);
        let active = root == Some(i);
        if active {
            fill(f.buffer_mut(), rect(area.x + 1, y, area.width.saturating_sub(2), 1), SEL);
            put(f.buffer_mut(), area.x, y, 1, "▌", accent());
        }
        let style = if active { bold() } else { muted() };
        let icon_style = if active { accent() } else { style };
        let icon = if i == NAV_DOWNLOADS && downloading { spin_frame() } else { icon };
        put(f.buffer_mut(), area.x + 2, y, 2, icon, icon_style);
        if !compact {
            let mut text = label.to_string();
            let up = app.queue.len().saturating_sub(app.qi + 1);
            if i == NAV_QUEUE && up > 0 {
                text = format!("{label}  {up}");
            }
            put(f.buffer_mut(), area.x + 5, y, area.width.saturating_sub(10), &text, style);
            let n = (i + 1).to_string();
            put(f.buffer_mut(), area.right().saturating_sub(3), y, 1, &n, Style::default().fg(FAINT));
        }
        app.hits.push((row, Hit::Nav(i)));
        y += 1;
    }

    // Now playing art fills whatever room is left.
    let footer = 2;
    if let (Some(song), false) = (app.current.clone(), compact) {
        let w = area.width.saturating_sub(4);
        let h = w / 2;
        let top = area.bottom().saturating_sub(h + footer + 3);
        if top > y + 1 && h >= 4 {
            put(f.buffer_mut(), area.x + 2, top, w, "NOW PLAYING", dim().add_modifier(Modifier::BOLD));
            let r = rect(area.x + 2, top + 1, w, h);
            draw_art(f, &mut app.art, song.cover.as_deref(), &song.album, r, false);
            app.hits.push((r, Hit::OpenNowPlaying));
            put(f.buffer_mut(), area.x + 2, top + h + 1, w, &song.title, bold());
        }
    }
    if !compact {
        let host = app
            .client
            .as_ref()
            .map(|c| c.server().trim_start_matches("https://").trim_start_matches("http://").to_string())
            .unwrap_or_default();
        let (dot, st) = if app.cfg.settings.offline_mode { ("○ offline", dim()) } else { ("●", accent()) };
        let x = area.x + 2;
        let by = area.bottom().saturating_sub(1);
        let w = put(f.buffer_mut(), x, by, 10, dot, st);
        let label = format!(" {} · {}", app.cfg.username, host);
        put(f.buffer_mut(), x + w, by, area.width.saturating_sub(3 + w), &label, dim());
    }
}

// ---------------------------------------------------------------- main

fn draw_main(f: &mut Frame, app: &mut App, area: Rect) {
    let inner = rect(area.x + 2, area.y + 1, area.width.saturating_sub(3), area.height.saturating_sub(1));
    let focused = app.overlay.is_none();
    let depth = app.stack.len();
    let crumbs: Vec<String> = app.stack.iter().map(|v| v.title.clone()).collect();
    let queue = app.queue.clone();
    let qi = app.qi;
    let current = app.current.clone();
    let lyrics = app.lyrics.clone();
    let position = app.status.position;
    let greeting = app.greeting.clone();
    let context = app.context.clone();
    let spectrum = app.spectrum;
    let recents = app.cfg.settings.recent_searches.clone();
    let viz = app.cfg.settings.visualizer;
    let show_quality = app.cfg.settings.show_audio_quality;
    let auto_scroll = app.cfg.settings.lyrics_auto_scroll;
    let settings_rows = match app.top().map(|v| &v.kind) {
        Some(Kind::Settings { page, .. }) => Some((app.settings_rows(*page), *page)),
        _ => None,
    };
    let confirm = app.confirm.map(|c| c.0);
    let eq_gains = app.cfg.settings.eq_gains;
    let eq_on = app.cfg.settings.eq_enabled;
    let mut sliders: Vec<(usize, Rect)> = Vec::new();
    let mut ctx = Ctx {
        art: &mut app.art,
        hits: &mut app.hits,
        downloaded: &app.downloaded_ids,
        playing: current.as_ref().map(|s| s.id.clone()),
        paused: app.status.paused,
    };
    let Some(view) = app.stack.last_mut() else { return };

    let mut body = inner;
    if depth > 1 {
        // Back, then where you are: Albums › Album name.
        let buf = f.buffer_mut();
        let bw = put(buf, inner.x, inner.y, 8, " ‹ Back ", Style::default().bg(CHIP).fg(INK));
        ctx.hit(rect(inner.x, inner.y, bw, 1), Hit::Back);
        let mut x = inner.x + bw + 2;
        for (d, t) in crumbs.iter().enumerate() {
            let last = d + 1 == crumbs.len();
            let t = fit(t, 28);
            let style = if last { bold() } else { muted() };
            let w = put(f.buffer_mut(), x, inner.y, inner.right().saturating_sub(x), &t, style);
            if !last {
                ctx.hit(rect(x, inner.y, w, 1), Hit::Crumb(d));
            }
            x += w;
            if !last {
                x += put(f.buffer_mut(), x, inner.y, 3, " › ", dim());
            }
            if x >= inner.right() {
                break;
            }
        }
        body = rect(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(2));
    }

    if let Some(e) = &view.error {
        if !matches!(view.kind, Kind::Search(_)) {
            put(f.buffer_mut(), body.x, body.y, body.width, "Couldn't load this page.", bold());
            put(f.buffer_mut(), body.x, body.y + 1, body.width, e, Style::default().fg(ERROR));
            put(f.buffer_mut(), body.x, body.y + 3, body.width, "Esc goes back. Pick it again in the sidebar to retry.", muted());
            return;
        }
    }

    match &mut view.kind {
        Kind::Home { shelves, row, scroll } => {
            put(f.buffer_mut(), body.x, body.y, body.width, &greeting, bold());
            let content = rect(body.x, body.y + 2, body.width, body.height.saturating_sub(2));
            if view.loading {
                spinner(f.buffer_mut(), content, "Loading your library…");
                return;
            }
            if shelves.is_empty() {
                put(f.buffer_mut(), content.x, content.y, content.width, "Your library is empty. Add music to your server and it shows up here.", muted());
                return;
            }
            draw_home(f, &mut ctx, shelves, row, scroll, content, focused);
        }
        Kind::Search(s) => draw_search(f, &mut ctx, s, &recents, view.error.as_deref(), body, focused),
        Kind::Grid { title, kind, grid, fetching, .. } => {
            let mut top = body.y;
            put(f.buffer_mut(), body.x, top, body.width, title, bold());
            let count = if grid.items.is_empty() { String::new() } else { format!("{} / {}", grid.sel + 1, grid.items.len()) };
            put(f.buffer_mut(), body.x + title.width() as u16 + 2, top, 14, &count, dim());
            if let GridKind::Albums(sort) = kind {
                let mut x = body.x + title.width() as u16 + 16;
                for (i, s) in AlbumSort::ALL.iter().enumerate() {
                    let label = format!(" {} ", s.label());
                    let w = label.width() as u16;
                    if x + w > body.right() {
                        break;
                    }
                    let style = if s == sort {
                        Style::default().bg(ACCENT()).fg(ACCENT_INK).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(CHIP).fg(INK)
                    };
                    put(f.buffer_mut(), x, top, w, &label, style);
                    ctx.hit(rect(x, top, w, 1), Hit::SortChip(i));
                    x += w + 1;
                }
            }
            top += 2;
            let content = rect(body.x.saturating_sub(1), top, body.width + 1, body.bottom().saturating_sub(top));
            if view.loading && grid.items.is_empty() {
                spinner(f.buffer_mut(), content, "Loading…");
                return;
            }
            if grid.items.is_empty() {
                put(f.buffer_mut(), content.x + 1, content.y, content.width, "Nothing here yet.", muted());
                return;
            }
            let h = art_rows(content);
            draw_grid(f, &mut ctx, grid, content, focused, h, Hit::GridItem);
            if *fetching {
                put(f.buffer_mut(), body.right().saturating_sub(12), body.y, 12, &format!("{} more…", spin_frame()), dim());
            }
        }
        Kind::Tracks { header, songs, sel, scroll, source } => {
            if view.loading {
                spinner(f.buffer_mut(), body, "Loading…");
                return;
            }
            let is_downloads = matches!(source, Source::Downloads);
            let all_dl = !songs.is_empty() && songs.iter().all(|s| ctx.downloaded.contains(&s.id));
            let mut top = body.y;
            if let Some(h) = header {
                let dl = if is_downloads { None } else { Some(all_dl) };
                top += draw_header(f, &mut ctx, h, body, dl);
            }
            let content = rect(body.x, top, body.width, body.bottom().saturating_sub(top));
            if songs.is_empty() {
                let msg = if is_downloads {
                    "Nothing downloaded yet. Press D on a song, album or playlist, or pick Download in its menu."
                } else {
                    "No songs here yet."
                };
                put(f.buffer_mut(), content.x, content.y + 1, content.width, msg, muted());
                return;
            }
            let show_album = !matches!(header, Some(h) if h.kind == "Album");
            track_table(f, &mut ctx, songs, *sel, scroll, content, focused, show_album, Hit::Track);
        }
        Kind::Artist { artist, top, top_sel, albums, section } => {
            if view.loading {
                spinner(f.buffer_mut(), body, "Loading…");
                return;
            }
            let Some(a) = artist else { return };
            let compact = *section == 1 || body.height < 30;
            let mut y = body.y;
            let h = Header {
                kind: "Artist",
                title: a.name.clone(),
                subtitle: format!(
                    "{} album{}{}",
                    albums.items.len(),
                    if albums.items.len() == 1 { "" } else { "s" },
                    crate::api::provider_name(&a.id).map(|p| format!(" · via {p}")).unwrap_or_default()
                ),
                cover: a.cover.clone(),
                seed: a.id.clone(),
                liked: false,
                star: Some((a.id.clone(), crate::api::StarKind::Artist, a.starred)),
            };
            y += if compact { draw_header_compact(f, &mut ctx, &h, body, None) } else { draw_header(f, &mut ctx, &h, body, None) };
            if !top.is_empty() {
                put(f.buffer_mut(), body.x, y, body.width, "Popular", bold());
                y += 1;
                let rows = if *section == 0 { top.len().min(10) } else { top.len().min(3) } as u16;
                let r = rect(body.x, y, body.width, rows + 1);
                let mut sc = 0usize;
                track_rows(f, &mut ctx, top, *top_sel, &mut sc, r, focused && *section == 0, true, Hit::TopSong, false);
                y += rows + 2;
            }
            if !albums.items.is_empty() && y + 6 < body.bottom() {
                put(f.buffer_mut(), body.x, y, body.width, "Discography", bold());
                y += 1;
                let r = rect(body.x.saturating_sub(1), y, body.width + 1, body.bottom().saturating_sub(y));
                let ah = art_rows(r).min(8).min(r.height.saturating_sub(4).max(4));
                draw_grid(f, &mut ctx, albums, r, focused && *section == 1, ah, Hit::ArtistAlbum);
            }
        }
        Kind::Queue { sel, scroll } => {
            draw_queue(f, &mut ctx, &queue, qi, &context, sel, scroll, body, focused);
        }
        Kind::NowPlaying => {
            if let Some(song) = current {
                now_playing(f, &mut ctx, &song, &queue, qi, lyrics.as_ref(), position, &spectrum, viz, show_quality, auto_scroll, &context, body);
            }
        }
        Kind::Settings { sel, scroll, .. } => {
            if let Some((rows, page)) = &settings_rows {
                draw_settings(f, &mut ctx, *page, rows, *sel, scroll, confirm, &spectrum, &eq_gains, eq_on, body, &mut sliders);
            }
        }
    }
    app.slider_rects = sliders;
}

fn spin_frame() -> &'static str {
    const F: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    F[(ms / 90) as usize % F.len()]
}

fn spinner(buf: &mut Buffer, area: Rect, label: &str) {
    put(buf, area.x, area.y, area.width, &format!("{} {label}", spin_frame()), muted());
}

fn draw_home(f: &mut Frame, ctx: &mut Ctx, shelves: &mut [Shelf], row: &mut usize, scroll: &mut usize, area: Rect, focused: bool) {
    let art_h = art_rows(area).min(8);
    let art_w = art_h * 2;
    let cw = art_w + 3;
    let sh = art_h + 6;
    let visible = (area.height / sh).max(1) as usize;
    *row = (*row).min(shelves.len() - 1);
    if *row < *scroll {
        *scroll = *row;
    }
    if *row >= *scroll + visible {
        *scroll = *row + 1 - visible;
    }
    let cols = ((area.width.saturating_sub(1)) / cw).max(1) as usize;
    for (k, ri) in (*scroll..shelves.len()).take(visible + 1).enumerate() {
        let y = area.y + k as u16 * sh;
        if y + 4 > area.bottom() {
            break;
        }
        let s = &mut shelves[ri];
        let active = ri == *row;
        let tw = put(f.buffer_mut(), area.x, y, area.width, &s.title, if active && focused { bold().fg(ACCENT()) } else { ink() });
        if active {
            put(f.buffer_mut(), area.x + tw + 2, y, 10, &format!("{} / {}", s.sel + 1, s.items.len()), dim());
        }
        if s.sel < s.offset {
            s.offset = s.sel;
        }
        if s.sel >= s.offset + cols {
            s.offset = s.sel + 1 - cols;
        }
        let more_left = s.offset > 0;
        let more_right = s.offset + cols < s.items.len();
        let arrows = format!("{}  {}", if more_left { "‹" } else { " " }, if more_right { "›" } else { " " });
        put(f.buffer_mut(), area.right().saturating_sub(5), y, 4, &arrows, muted());
        for c in 0..cols {
            let i = s.offset + c;
            let Some(item) = s.items.get(i) else { break };
            let x = area.x.saturating_sub(1) + c as u16 * cw;
            if y + 1 + art_h + 2 > area.bottom() {
                break;
            }
            card(f, ctx, item, x, y + 1, art_h, active && i == s.sel, focused, Hit::Shelf(ri, i));
        }
    }
    if shelves.len() > visible {
        scrollbar(f.buffer_mut(), area, *scroll, visible, shelves.len());
    }
}

// ---------------------------------------------------------------- search

fn draw_search(f: &mut Frame, ctx: &mut Ctx, s: &mut SearchState, recents: &[String], error: Option<&str>, body: Rect, focused: bool) {
    let box_r = rect(body.x, body.y, body.width.min(72), 3);
    let typing = s.editing && focused;
    let border = if typing { ACCENT() } else { FAINT };
    let b = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(CHIP));
    f.render_widget(b, box_r);
    let buf = f.buffer_mut();
    put(buf, box_r.x + 2, box_r.y + 1, 2, "⌕", if typing { accent() } else { muted() });
    let w = if s.query.is_empty() {
        put(buf, box_r.x + 4, box_r.y + 1, box_r.width - 6, "Songs, artists, albums or playlists", dim());
        0
    } else {
        put(buf, box_r.x + 4, box_r.y + 1, box_r.width - 8, &s.query, ink())
    };
    if typing {
        let cx = if s.query.is_empty() { box_r.x + 4 } else { box_r.x + 4 + w };
        put(buf, cx, box_r.y + 1, 1, "▏", accent());
    }
    if s.busy {
        put(buf, box_r.right().saturating_sub(4), box_r.y + 1, 2, spin_frame(), accent());
    } else if !s.query.is_empty() && typing {
        put(buf, box_r.right().saturating_sub(12), box_r.y + 1, 10, "esc clears", Style::default().fg(FAINT).bg(CHIP));
    }
    ctx.hit(box_r, Hit::SearchBox);

    let area = rect(body.x, body.y + 4, body.width, body.height.saturating_sub(4));
    if s.searched.is_empty() {
        if recents.is_empty() {
            put(f.buffer_mut(), area.x, area.y, area.width, "Start typing. Results update as you go.", bold());
            put(f.buffer_mut(), area.x, area.y + 1, area.width, "Your library comes first, then anything octo-fiesta finds through its providers.", muted());
            put(f.buffer_mut(), area.x, area.y + 3, area.width, "↓ or Enter moves into the results. Esc in the results comes back up here.", dim());
            return;
        }
        put(f.buffer_mut(), area.x, area.y, area.width, "Recent searches", bold());
        for (i, q) in recents.iter().enumerate() {
            let y = area.y + 2 + i as u16;
            if y >= area.bottom() {
                break;
            }
            let row = rect(area.x, y, area.width.min(60), 1);
            let sel = !s.editing && s.recent_sel == i && focused;
            if sel {
                fill(f.buffer_mut(), row, SEL);
                put(f.buffer_mut(), area.x, y, 1, "▌", accent());
            }
            put(f.buffer_mut(), area.x + 2, y, 2, "↺", dim());
            put(f.buffer_mut(), area.x + 5, y, row.width.saturating_sub(6), q, if sel { bold() } else { ink() });
            ctx.hit(row, Hit::Recent(i));
        }
        let y = area.y + 3 + recents.len() as u16;
        put(f.buffer_mut(), area.x, y, area.width, "Enter searches again. x forgets one.", Style::default().fg(FAINT));
        return;
    }
    if let Some(e) = error {
        put(f.buffer_mut(), area.x, area.y, area.width, "Search failed.", bold());
        put(f.buffer_mut(), area.x, area.y + 1, area.width, e, Style::default().fg(ERROR));
        return;
    }
    let Some(r) = &s.ranked else {
        spinner(f.buffer_mut(), area, "Searching…");
        return;
    };
    let any = (SEC_TOP..=SEC_PLAYLISTS).any(|x| s.has(x));
    if !any {
        put(f.buffer_mut(), area.x, area.y, area.width, &format!("Nothing found for \"{}\".", s.searched), bold());
        put(f.buffer_mut(), area.x, area.y + 1, area.width, "Check the spelling, or try fewer or different words.", muted());
        return;
    }
    let res_focus = focused && !s.editing;
    let art_h = if area.height >= 34 { 6 } else { 4 };
    let card_h = art_h + 4;

    // Blocks, top to bottom: top result and songs, then one shelf each.
    let mut blocks: Vec<(usize, u16)> = Vec::new();
    if s.has(SEC_TOP) || s.has(SEC_SONGS) {
        let songs_h = s.song_rows() as u16 + 2;
        let top_h = if s.has(SEC_TOP) { art_h + 4 } else { 0 };
        blocks.push((SEC_TOP, top_h.max(songs_h)));
    }
    for sec in [SEC_ARTISTS, SEC_ALBUMS, SEC_PLAYLISTS] {
        if s.has(sec) {
            blocks.push((sec, card_h + 2));
        }
    }
    let sel_block = blocks.iter().position(|(b, _)| *b == s.sec || (s.sec <= SEC_SONGS && *b == SEC_TOP)).unwrap_or(0);
    let total: u16 = blocks.iter().map(|b| b.1).sum();
    let mut start = 0;
    if total > area.height {
        start = sel_block;
        // Show as much above the selection as still fits.
        let mut used: u16 = blocks[sel_block..].iter().map(|b| b.1).sum::<u16>().min(area.height);
        while start > 0 && used + blocks[start - 1].1 <= area.height {
            start -= 1;
            used += blocks[start].1;
        }
    }
    let mut y = area.y;
    for (sec, h) in blocks.iter().skip(start) {
        if y + 3 > area.bottom() {
            break;
        }
        let bh = (*h).min(area.bottom() - y);
        if *sec == SEC_TOP {
            let left_w = if s.has(SEC_TOP) { (area.width * 2 / 5).clamp(30, 52).min(area.width) } else { 0 };
            if let Some(top) = &r.top {
                let sel = res_focus && s.sec == SEC_TOP;
                put(f.buffer_mut(), area.x, y, left_w, "Top result", bold());
                let cr = rect(area.x, y + 1, left_w.saturating_sub(2), art_h + 2);
                fill(f.buffer_mut(), cr, if sel { SEL } else { CARD });
                if sel {
                    for k in 0..cr.height {
                        put(f.buffer_mut(), cr.x, cr.y + k, 1, "▌", accent());
                    }
                }
                let ar = rect(cr.x + 2, cr.y + 1, art_h * 2, art_h);
                draw_art(f, ctx.art, top.cover(), top.id(), ar, false);
                let tx = ar.right() + 2;
                let tw = cr.right().saturating_sub(tx + 1);
                let buf = f.buffer_mut();
                put(buf, tx, cr.y + 1, tw, top.title(), if sel { bold().fg(ACCENT()) } else { bold() });
                let chip = format!(" {} ", top.kind_label());
                let cw = put(buf, tx, cr.y + 3, tw, &chip, Style::default().bg(SIDEBAR).fg(INK));
                let sub = top.subtitle();
                if sub != top.kind_label() {
                    put(buf, tx + cw + 1, cr.y + 3, tw.saturating_sub(cw + 1), &sub, muted());
                }
                let action = if matches!(top, Item::Song(_)) { "enter plays" } else { "enter opens" };
                put(buf, tx, cr.bottom().saturating_sub(2), tw, action, Style::default().fg(FAINT));
                ctx.hit(cr, Hit::SearchTop);
            }
            if s.has(SEC_SONGS) {
                let sx = area.x + left_w + if left_w > 0 { 1 } else { 0 };
                let sw = area.right().saturating_sub(sx);
                put(f.buffer_mut(), sx, y, sw, &format!("Songs  {}", r.songs.len()), bold());
                let rows = s.song_rows();
                for i in 0..rows {
                    let ry = y + 1 + i as u16;
                    if ry >= y + bh {
                        break;
                    }
                    let row = rect(sx, ry, sw.saturating_sub(1), 1);
                    let sel = res_focus && s.sec == SEC_SONGS && s.song_sel == i;
                    if sel {
                        fill(f.buffer_mut(), row, SEL);
                        put(f.buffer_mut(), sx, ry, 1, "▌", accent());
                    }
                    if i >= SEARCH_SONGS {
                        let t = format!("See all {} songs ›", r.songs.len());
                        put(f.buffer_mut(), sx + 2, ry, sw.saturating_sub(3), &t, if sel { bold().fg(ACCENT()) } else { muted() });
                        ctx.hit(row, Hit::SearchSeeAll);
                        continue;
                    }
                    let song = &r.songs[i];
                    let playing = ctx.playing.as_deref() == Some(song.id.as_str());
                    let buf = f.buffer_mut();
                    let mark = if playing { if ctx.paused { "❚" } else { eq_frame() } } else { "" };
                    put(buf, sx + 2, ry, 2, mark, accent());
                    let dur = fmt_time(song.duration as f64);
                    let avail = sw.saturating_sub(4 + 9);
                    let mut title = song.title.clone();
                    if let Some(p) = crate::api::provider_name(&song.id) {
                        title = format!("{title}  ◆{p}");
                    }
                    let st = if playing {
                        bold().fg(ACCENT())
                    } else if sel {
                        bold()
                    } else {
                        ink()
                    };
                    let col = avail * 6 / 10;
                    put(buf, sx + 4, ry, col.saturating_sub(1), &title, st);
                    put(buf, sx + 4 + col, ry, avail.saturating_sub(col), &song.artist, muted());
                    if ctx.downloaded.contains(&song.id) {
                        put(buf, row.right().saturating_sub(8), ry, 1, "↓", accent());
                    }
                    put_right(buf, row.right(), ry, 6, &dur, dim());
                    ctx.hit(row, Hit::SearchSong(i));
                }
            }
        } else {
            let i = sec - SEC_ARTISTS;
            let items = s.shelf_items(*sec);
            let name = match *sec {
                SEC_ARTISTS => "Artists",
                SEC_ALBUMS => "Albums",
                _ => "Playlists",
            };
            let active = res_focus && s.sec == *sec;
            let tw = put(f.buffer_mut(), area.x, y, area.width, name, if active { bold().fg(ACCENT()) } else { bold() });
            put(f.buffer_mut(), area.x + tw + 2, y, 12, &format!("{}", items.len()), dim());
            let cw = art_h * 2 + 3;
            let cols = (area.width.saturating_sub(1) / cw).max(1) as usize;
            let (sel, off) = &mut s.shelf[i];
            *sel = (*sel).min(items.len().saturating_sub(1));
            if *sel < *off {
                *off = *sel;
            }
            if *sel >= *off + cols {
                *off = *sel + 1 - cols;
            }
            let more = format!("{}  {}", if *off > 0 { "‹" } else { " " }, if *off + cols < items.len() { "›" } else { " " });
            put(f.buffer_mut(), area.right().saturating_sub(5), y, 4, &more, muted());
            let (sel, off) = (*sel, *off);
            for c in 0..cols {
                let k = off + c;
                let Some(item) = items.get(k) else { break };
                let x = area.x.saturating_sub(1) + c as u16 * cw;
                if y + 1 + card_h > area.bottom() + 1 {
                    break;
                }
                card(f, ctx, item, x, y + 1, art_h, active && k == sel, res_focus, Hit::SearchShelf(*sec, k));
            }
        }
        y += bh;
    }
}

// ---------------------------------------------------------------- headers and rows

fn button(buf: &mut Buffer, x: u16, y: u16, label: &str, primary: bool) -> u16 {
    let style = if primary {
        Style::default().bg(ACCENT()).fg(ACCENT_INK).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(CHIP).fg(INK)
    };
    let t = format!(" {label} ");
    put(buf, x, y, t.width() as u16, &t, style)
}

fn header_buttons(f: &mut Frame, ctx: &mut Ctx, h: &Header, x: u16, y: u16, download: Option<bool>) {
    let mut bx = x;
    let w = button(f.buffer_mut(), bx, y, "▶ Play", true);
    ctx.hit(rect(bx, y, w, 1), Hit::HeaderPlay);
    bx += w + 1;
    let w = button(f.buffer_mut(), bx, y, "⇄ Shuffle", false);
    ctx.hit(rect(bx, y, w, 1), Hit::HeaderShuffle);
    bx += w + 1;
    if let Some((_, kind, on)) = &h.star {
        let label = match (kind, on) {
            (crate::api::StarKind::Artist, true) => "♥ Following",
            (crate::api::StarKind::Artist, false) => "♡ Follow",
            (_, true) => "♥ Liked",
            (_, false) => "♡ Like",
        };
        let w = button(f.buffer_mut(), bx, y, label, false);
        if *on {
            put(f.buffer_mut(), bx + 1, y, 1, "♥", Style::default().bg(CHIP).fg(ACCENT()));
        }
        ctx.hit(rect(bx, y, w, 1), Hit::HeaderStar);
        bx += w + 1;
    }
    if let Some(done) = download {
        let label = if done { "✓ Downloaded" } else { "↓ Download" };
        let w = button(f.buffer_mut(), bx, y, label, false);
        if done {
            put(f.buffer_mut(), bx + 1, y, 1, "✓", Style::default().bg(CHIP).fg(ACCENT()));
        }
        ctx.hit(rect(bx, y, w, 1), Hit::HeaderDownload);
    }
}

/// Returns the height used.
fn draw_header(f: &mut Frame, ctx: &mut Ctx, h: &Header, area: Rect, download: Option<bool>) -> u16 {
    if area.height < 22 {
        return draw_header_compact(f, ctx, h, area, download);
    }
    let art_h = 10.min(area.height / 3);
    let art_w = art_h * 2;
    let ar = rect(area.x, area.y, art_w, art_h);
    draw_art(f, ctx.art, h.cover.as_deref(), &h.seed, ar, h.liked);
    let tx = area.x + art_w + 3;
    let tw = area.right().saturating_sub(tx);
    let base = area.y + art_h;
    let buf = f.buffer_mut();
    put(buf, tx, base.saturating_sub(6), tw, &h.kind.to_uppercase(), muted().add_modifier(Modifier::BOLD));
    put(buf, tx, base.saturating_sub(5), tw, &h.title, bold());
    put(buf, tx, base.saturating_sub(3), tw, &h.subtitle, muted());
    header_buttons(f, ctx, h, tx, base.saturating_sub(1), download);
    art_h + 2
}

fn draw_header_compact(f: &mut Frame, ctx: &mut Ctx, h: &Header, area: Rect, download: Option<bool>) -> u16 {
    let ar = rect(area.x, area.y, 6, 3);
    draw_art(f, ctx.art, h.cover.as_deref(), &h.seed, ar, h.liked);
    let tx = area.x + 8;
    let tw = area.right().saturating_sub(tx);
    let buf = f.buffer_mut();
    let kw = put(buf, tx, area.y, tw, &h.kind.to_uppercase(), dim().add_modifier(Modifier::BOLD));
    put(buf, tx + kw + 2, area.y, tw.saturating_sub(kw + 2), &h.subtitle, muted());
    put(buf, tx, area.y + 1, tw, &h.title, bold());
    header_buttons(f, ctx, h, tx, area.y + 2, download);
    4
}

#[allow(clippy::too_many_arguments)]
fn track_table(f: &mut Frame, ctx: &mut Ctx, songs: &[Song], sel: usize, scroll: &mut usize, area: Rect, focused: bool, show_album: bool, hit: fn(usize) -> Hit) {
    track_rows(f, ctx, songs, sel, scroll, area, focused, show_album, hit, true);
}

#[allow(clippy::too_many_arguments)]
fn track_rows(f: &mut Frame, ctx: &mut Ctx, songs: &[Song], sel: usize, scroll: &mut usize, area: Rect, focused: bool, show_album: bool, hit: fn(usize) -> Hit, header: bool) {
    if area.height < 2 || songs.is_empty() {
        return;
    }
    let w = area.width.saturating_sub(1);
    let idx_w = 5u16;
    let dur_w = 6u16;
    let mark_w = 4u16;
    let rest = w.saturating_sub(idx_w + dur_w + mark_w + 2);
    let (title_w, artist_w, album_w) = if show_album && rest > 70 {
        (rest * 42 / 100, rest * 26 / 100, rest - rest * 42 / 100 - rest * 26 / 100)
    } else {
        (rest * 60 / 100, rest - rest * 60 / 100, 0)
    };
    let mut y = area.y;
    if header {
        let buf = f.buffer_mut();
        let hs = dim();
        put(buf, area.x, y, idx_w, "   #", hs);
        let mut x = area.x + idx_w;
        put(buf, x, y, title_w, "Title", hs);
        x += title_w;
        put(buf, x, y, artist_w, "Artist", hs);
        x += artist_w;
        if album_w > 0 {
            put(buf, x, y, album_w, "Album", hs);
            x += album_w;
        }
        put(buf, x + mark_w, y, dur_w, "  time", hs);
        y += 1;
        let line: String = "─".repeat(w as usize);
        put(buf, area.x, y, w, &line, Style::default().fg(FAINT));
        y += 1;
    }
    let rows = area.bottom().saturating_sub(y) as usize;
    if rows == 0 {
        return;
    }
    let sel = sel.min(songs.len() - 1);
    if sel < *scroll {
        *scroll = sel;
    }
    if sel >= *scroll + rows {
        *scroll = sel + 1 - rows;
    }
    for (k, i) in (*scroll..songs.len()).take(rows).enumerate() {
        let s = &songs[i];
        let ry = y + k as u16;
        let row = rect(area.x, ry, w, 1);
        let selected = i == sel;
        if selected {
            fill(f.buffer_mut(), row, if focused { SEL } else { CARD });
            if focused {
                put(f.buffer_mut(), area.x, ry, 1, "▌", accent());
            }
        }
        let playing = ctx.playing.as_deref() == Some(s.id.as_str());
        let buf = f.buffer_mut();
        let num = if playing {
            if ctx.paused { "  ❚ ".to_string() } else { format!("  {} ", eq_frame()) }
        } else {
            format!("{:>3} ", if header { i + 1 } else { k + *scroll + 1 })
        };
        put(buf, area.x + 1, ry, idx_w - 1, &num, if playing { accent() } else { dim() });
        let mut x = area.x + idx_w;
        let title_style = if playing {
            accent()
        } else if selected && focused {
            bold()
        } else {
            ink()
        };
        let mut title = s.title.clone();
        if let Some(p) = crate::api::provider_name(&s.id) {
            title = format!("{title}  ◆{p}");
        }
        if s.explicit {
            title = format!("{title} [E]");
        }
        put(buf, x, ry, title_w.saturating_sub(1), &title, title_style);
        x += title_w;
        put(buf, x, ry, artist_w.saturating_sub(1), &s.artist, muted());
        x += artist_w;
        if album_w > 0 {
            put(buf, x, ry, album_w.saturating_sub(1), &s.album, muted());
            x += album_w;
        }
        if ctx.downloaded.contains(&s.id) {
            put(buf, x, ry, 1, "↓", accent());
        }
        if s.starred {
            put(buf, x + 2, ry, 1, "♥", accent());
        }
        x += mark_w;
        put(buf, x, ry, dur_w, &format!("{:>5}", fmt_time(s.duration as f64)), dim());
        ctx.hit(row, hit(i));
    }
    if songs.len() > rows {
        scrollbar(f.buffer_mut(), rect(area.x, y, area.width, rows as u16), *scroll, rows, songs.len());
    }
}

fn eq_frame() -> &'static str {
    const F: [&str; 6] = ["▂", "▄", "▆", "█", "▆", "▄"];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    F[(ms / 220) as usize % F.len()]
}

// ---------------------------------------------------------------- queue

#[allow(clippy::too_many_arguments)]
fn draw_queue(f: &mut Frame, ctx: &mut Ctx, queue: &[Song], qi: usize, context: &str, sel: &mut usize, scroll: &mut usize, body: Rect, focused: bool) {
    let tw = put(f.buffer_mut(), body.x, body.y, body.width, "Queue", bold());
    if queue.is_empty() {
        put(f.buffer_mut(), body.x, body.y + 2, body.width, "The queue is empty. Press a on any song, album or playlist to add it.", muted());
        return;
    }
    let left: u32 = queue.iter().skip(qi).map(|s| s.duration).sum();
    let up = queue.len().saturating_sub(qi + 1);
    let sub = format!("{up} up next · {} left", fmt_long(left));
    put(f.buffer_mut(), body.x + tw + 2, body.y, body.width, &sub, dim());
    if !context.is_empty() {
        let cx = put(f.buffer_mut(), body.x, body.y + 1, 14, "Playing from ", muted());
        put(f.buffer_mut(), body.x + cx, body.y + 1, body.width.saturating_sub(cx), context, ink());
    }
    let by = body.y + 3;
    let mut bx = body.x;
    for (label, hit) in [
        ("⇄ Shuffle up next", Hit::QueueShuffle),
        ("⌫ Clear up next", Hit::QueueClear),
        ("≡ Save as playlist", Hit::QueueSave),
    ] {
        let w = button(f.buffer_mut(), bx, by, label, false);
        ctx.hit(rect(bx, by, w, 1), hit);
        bx += w + 1;
    }
    put(f.buffer_mut(), bx + 1, by, body.right().saturating_sub(bx + 1), "drag a row to move it", Style::default().fg(FAINT));
    let r = rect(body.x, by + 2, body.width, body.height.saturating_sub(5));
    *sel = (*sel).min(queue.len() - 1);
    let rows = r.height as usize;
    if rows == 0 {
        return;
    }
    if *sel < *scroll {
        *scroll = *sel;
    }
    if *sel >= *scroll + rows {
        *scroll = *sel + 1 - rows;
    }
    let w = r.width.saturating_sub(1);
    for (k, i) in (*scroll..queue.len()).take(rows).enumerate() {
        let s = &queue[i];
        let y = r.y + k as u16;
        let row = rect(r.x, y, w, 1);
        if i == *sel {
            fill(f.buffer_mut(), row, if focused { SEL } else { CARD });
            if focused {
                put(f.buffer_mut(), r.x, y, 1, "▌", accent());
            }
        }
        let buf = f.buffer_mut();
        let (mark, style) = if i == qi {
            (if ctx.paused { "  ❚ ".to_string() } else { format!("  {} ", eq_frame()) }, accent())
        } else if i < qi {
            ("  ✓ ".to_string(), Style::default().fg(FAINT))
        } else {
            (format!("{:>3} ", i - qi), dim())
        };
        put(buf, r.x + 1, y, 5, &mark, style);
        let tstyle = if i == qi {
            accent()
        } else if i < qi {
            dim()
        } else {
            ink()
        };
        let tw = w.saturating_sub(6 + 10) * 55 / 100;
        put(buf, r.x + 6, y, tw.saturating_sub(1), &s.title, tstyle);
        put(buf, r.x + 6 + tw, y, w.saturating_sub(6 + tw + 10), &s.artist, muted());
        if ctx.downloaded.contains(&s.id) {
            put(buf, r.x + w.saturating_sub(9), y, 1, "↓", accent());
        }
        put(buf, r.x + w.saturating_sub(6), y, 6, &format!("{:>5}", fmt_time(s.duration as f64)), dim());
        ctx.hit(row, Hit::QueueRow(i));
    }
    scrollbar(f.buffer_mut(), r, *scroll, rows, queue.len());
}

// ---------------------------------------------------------------- visualizer

/// The spectrum resampled to `n` columns.
fn columns(spec: &[f32; BANDS], n: usize) -> Vec<f32> {
    (0..n)
        .map(|c| {
            let p = if n <= 1 { 0.0 } else { c as f32 / (n - 1) as f32 * (BANDS - 1) as f32 };
            let i = p.floor() as usize;
            let t = p - i as f32;
            let a = spec[i.min(BANDS - 1)];
            let b = spec[(i + 1).min(BANDS - 1)];
            a + (b - a) * t
        })
        .collect()
}

const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

fn viz_color(row_frac: f32) -> Color {
    let a = accent_rgb();
    let dark = (a.0 / 5, a.1 / 5, a.2 / 5);
    theme::mix(dark, a, (0.35 + 0.65 * row_frac).clamp(0.0, 1.0))
}

fn draw_viz(buf: &mut Buffer, spec: &[f32; BANDS], style: Visualizer, r: Rect) {
    if r.width < 4 || r.height == 0 {
        return;
    }
    match style {
        Visualizer::Off => {}
        Visualizer::Ribbon => {
            let cols = columns(spec, r.width as usize);
            for (c, v) in cols.iter().enumerate() {
                let lvl = (v * 8.0).round().clamp(1.0, 8.0) as usize;
                put(buf, r.x + c as u16, r.y + r.height - 1, 1, EIGHTHS[lvl], Style::default().fg(viz_color(*v)));
            }
        }
        Visualizer::Bars => {
            let n = (r.width as usize / 3).max(4);
            let cols = columns(spec, n);
            let h = r.height as f32;
            for (b, v) in cols.iter().enumerate() {
                let total = v * h * 8.0;
                for row in 0..r.height {
                    let from_bottom = (r.height - 1 - row) as f32;
                    let e = (total - from_bottom * 8.0).clamp(0.0, 8.0) as usize;
                    if e == 0 {
                        continue;
                    }
                    let s = EIGHTHS[e].repeat(2);
                    let st = Style::default().fg(viz_color(from_bottom / h.max(1.0)));
                    put(buf, r.x + b as u16 * 3, r.y + row, 2, &s, st);
                }
            }
        }
        Visualizer::Mirror => {
            let n = (r.width as usize / 2).max(4);
            let cols = columns(spec, n);
            let half = r.height as f32 / 2.0;
            let mid = r.y as f32 + half;
            for (b, v) in cols.iter().enumerate() {
                let ext = v * half;
                for row in 0..r.height {
                    let center = r.y as f32 + row as f32 + 0.5;
                    let d = (center - mid).abs();
                    let st = Style::default().fg(viz_color(1.0 - d / half.max(1.0)));
                    let s = if d + 0.5 <= ext {
                        "█"
                    } else if d <= ext {
                        if center < mid { "▄" } else { "▀" }
                    } else {
                        continue;
                    };
                    put(buf, r.x + b as u16 * 2, r.y + row, 1, s, st);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- now playing

#[allow(clippy::too_many_arguments)]
fn now_playing(
    f: &mut Frame,
    ctx: &mut Ctx,
    song: &Song,
    queue: &[Song],
    qi: usize,
    lyrics: Option<&(String, Option<Lyrics>)>,
    position: f64,
    spec: &[f32; BANDS],
    viz: Visualizer,
    show_quality: bool,
    auto_scroll: bool,
    context: &str,
    area: Rect,
) {
    let max_h = area.height.saturating_sub(1);
    let art_h = max_h.min(area.width / 4).max(4);
    let art_w = art_h * 2;
    let ar = rect(area.x, area.y, art_w, art_h);
    draw_art(f, ctx.art, song.cover.as_deref(), &song.album, ar, false);
    ctx.hit(ar, Hit::Toggle);

    let tx = area.x + art_w + 4;
    let tw = area.right().saturating_sub(tx + 1);
    if tw < 10 {
        return;
    }
    let buf = f.buffer_mut();
    let mut y = area.y;
    let label = if context.is_empty() { "NOW PLAYING".to_string() } else { format!("PLAYING FROM {}", context.to_uppercase()) };
    put(buf, tx, y, tw, &label, dim().add_modifier(Modifier::BOLD));
    y += 2;
    put(buf, tx, y, tw, &song.title, bold().fg(ACCENT()));
    y += 1;
    put(buf, tx, y, tw, &song.artist, ink());
    y += 1;
    let mut meta = vec![song.album.clone()];
    if let Some(yr) = song.year.filter(|y| *y > 0) {
        meta.push(yr.to_string());
    }
    put(buf, tx, y, tw, &meta.join(" · "), muted());
    y += 1;
    if show_quality {
        let mut fmt = Vec::new();
        if let Some(s) = &song.suffix {
            fmt.push(s.to_uppercase());
        }
        if let Some(b) = song.bitrate.filter(|b| *b > 0) {
            fmt.push(format!("{b} kbps"));
        }
        if ctx.downloaded.contains(&song.id) {
            fmt.push("downloaded".into());
        }
        if let Some(p) = crate::api::provider_name(&song.id) {
            fmt.push(format!("via {p}"));
        }
        put(buf, tx, y, tw, &fmt.join(" · "), dim());
    }
    y += 2;

    let room = area.bottom().saturating_sub(y);
    if viz != Visualizer::Off && room > 10 {
        let vh = match viz {
            Visualizer::Ribbon => 2,
            _ => (room / 3).clamp(4, 9),
        };
        draw_viz(f.buffer_mut(), spec, viz, rect(tx, y, tw, vh));
        y += vh + 1;
    }

    let buf = f.buffer_mut();
    let panel = rect(tx, y, tw, area.bottom().saturating_sub(y));
    match lyrics {
        Some((id, Some(l))) if *id == song.id => {
            put(buf, tx, y, tw, "Lyrics", bold());
            let r = rect(tx, y + 2, tw, panel.height.saturating_sub(2));
            draw_lyrics(buf, l, position, auto_scroll, r);
        }
        _ => {
            let label = if lyrics.is_none() { "Up next   (looking for lyrics…)" } else { "Up next" };
            put(buf, tx, y, tw, label, bold());
            let mut ry = y + 2;
            for (k, s) in queue.iter().enumerate().skip(qi + 1).take(panel.height.saturating_sub(2) as usize) {
                let n = format!("{:>2}", k - qi);
                put(buf, tx, ry, 3, &n, dim());
                let w = put(buf, tx + 4, ry, tw.saturating_sub(4) * 6 / 10, &s.title, ink());
                put(buf, tx + 6 + w, ry, tw.saturating_sub(6 + w), &s.artist, muted());
                ry += 1;
            }
            if qi + 1 >= queue.len() {
                put(buf, tx, ry, tw, "Nothing queued after this song.", dim());
            }
        }
    }
}

fn draw_lyrics(buf: &mut Buffer, l: &Lyrics, position: f64, auto_scroll: bool, r: Rect) {
    if r.height == 0 {
        return;
    }
    match l {
        Lyrics::Synced(lines) => {
            let cur = lines.iter().rposition(|(t, _)| *t <= position + 0.25);
            let center = r.height as i64 / 3;
            // Off, the page stays put and only the current line lights up.
            let start = if auto_scroll { cur.map(|c| c as i64 - center).unwrap_or(0) } else { 0 };
            for row in 0..r.height as i64 {
                let i = start + row;
                if i < 0 || i as usize >= lines.len() {
                    continue;
                }
                let i = i as usize;
                let text = if lines[i].1.trim().is_empty() { "♪" } else { lines[i].1.as_str() };
                let style = match cur {
                    Some(c) if c == i => bold(),
                    Some(c) if i < c => Style::default().fg(FAINT),
                    _ => muted(),
                };
                put(buf, r.x, r.y + row as u16, r.width, text, style);
            }
        }
        Lyrics::Plain(lines) => {
            for (k, line) in lines.iter().take(r.height as usize).enumerate() {
                put(buf, r.x, r.y + k as u16, r.width, line, muted());
            }
        }
    }
}

// ---------------------------------------------------------------- settings

fn row_height(row: &SRow, width: u16) -> u16 {
    match row {
        SRow::Header(t) => {
            if t.is_empty() {
                1
            } else {
                2
            }
        }
        SRow::Account(..) => 4,
        SRow::Toggle(_, _, _, Some(n)) | SRow::Choice(_, _, _, Some(n)) => 1 + wrap(n, width.saturating_sub(6) as usize).len() as u16,
        SRow::Note(n) => wrap(n, width.saturating_sub(4) as usize).len().max(1) as u16,
        SRow::EqGraph => 9,
        _ => 1,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_settings(
    f: &mut Frame,
    ctx: &mut Ctx,
    page: SPage,
    rows: &[SRow],
    sel: usize,
    scroll: &mut usize,
    confirm: Option<crate::app::prefs::SKey>,
    spec: &[f32; BANDS],
    gains: &[f32; 10],
    eq_on: bool,
    body: Rect,
    sliders: &mut Vec<(usize, Rect)>,
) {
    let w = body.width.min(96);
    put(f.buffer_mut(), body.x, body.y, w, page.title(), bold());
    let area = rect(body.x, body.y + 2, w, body.height.saturating_sub(2));
    if rows.is_empty() {
        return;
    }
    let heights: Vec<u16> = rows.iter().map(|r| row_height(r, w)).collect();
    let sel = sel.min(rows.len() - 1);
    // Keep the selected row on screen.
    if sel < *scroll {
        *scroll = sel;
    }
    while *scroll < sel && heights[*scroll..=sel].iter().sum::<u16>() > area.height {
        *scroll += 1;
    }
    let mut y = area.y;
    let right = area.right();
    for (i, row) in rows.iter().enumerate().skip(*scroll) {
        let h = heights[i];
        if y + h > area.bottom() {
            break;
        }
        let selected = i == sel;
        let line = rect(area.x, y, w, 1);
        if selected && row.selectable() {
            let hh = if matches!(row, SRow::Account(..)) { 3 } else { 1 };
            fill(f.buffer_mut(), rect(area.x, y, w, hh), SEL);
            for k in 0..hh {
                put(f.buffer_mut(), area.x, y + k, 1, "▌", accent());
            }
        }
        let lx = area.x + 2;
        let buf = f.buffer_mut();
        match row {
            SRow::Header(t) => {
                if !t.is_empty() {
                    put(buf, area.x, y + 1, w, &t.to_uppercase(), dim().add_modifier(Modifier::BOLD));
                }
            }
            SRow::Account(user, host) => {
                let av = rect(lx, y, 6, 3);
                placeholder(buf, user, av, false);
                put(buf, lx + 8, y, w.saturating_sub(12), user, bold());
                put(buf, lx + 8, y + 1, w.saturating_sub(12), &format!("{user} · {host}"), muted());
                put(buf, lx + 8, y + 2, w.saturating_sub(12), "Account and server", dim());
                put_right(buf, right - 1, y + 1, 2, "›", muted());
                ctx.hit(rect(area.x, y, w, 3), Hit::SettingRow(i));
            }
            SRow::Nav(p, sub) => {
                put(buf, lx, y, w / 2, p.title(), if selected { bold() } else { ink() });
                put_right(buf, right - 3, y, w / 2, sub, muted());
                put_right(buf, right - 1, y, 2, "›", muted());
                ctx.hit(line, Hit::SettingRow(i));
            }
            SRow::Toggle(_, label, on, note) => {
                put(buf, lx, y, w.saturating_sub(12), label, if selected { bold() } else { ink() });
                let (t, st) = if *on {
                    ("  On ● ", Style::default().bg(ACCENT()).fg(ACCENT_INK).add_modifier(Modifier::BOLD))
                } else {
                    (" ● Off  ", Style::default().bg(FAINT).fg(INK))
                };
                put_right(buf, right - 1, y, 8, t, st);
                if let Some(n) = note {
                    for (k, l) in wrap(n, w.saturating_sub(6) as usize).iter().enumerate() {
                        put(buf, lx, y + 1 + k as u16, w.saturating_sub(4), l, dim());
                    }
                }
                ctx.hit(rect(area.x, y, w, h), Hit::SettingRow(i));
            }
            SRow::Choice(_, label, value, note) => {
                put(buf, lx, y, w / 2, label, if selected { bold() } else { ink() });
                let v = format!("‹ {value} ›");
                put_right(buf, right - 1, y, w / 2, &v, if selected { bold().fg(ACCENT()) } else { muted() });
                if let Some(n) = note {
                    for (k, l) in wrap(n, w.saturating_sub(6) as usize).iter().enumerate() {
                        put(buf, lx, y + 1 + k as u16, w.saturating_sub(4), l, dim());
                    }
                }
                ctx.hit(rect(area.x, y, w, h), Hit::SettingRow(i));
            }
            SRow::Slider(_, label, frac, value) => {
                let lw = 18u16.min(w / 3);
                put(buf, lx, y, lw, label, if selected { bold() } else { ink() });
                let vw = 9u16;
                let bx = lx + lw + 1;
                let bw = w.saturating_sub(lw + vw + 6).max(4);
                let knob = ((frac.clamp(0.0, 1.0) * bw.saturating_sub(1) as f32).round() as u16).min(bw.saturating_sub(1));
                for k in 0..bw {
                    let (s, st) = if k < knob {
                        ("━", Style::default().fg(ACCENT()))
                    } else if k == knob {
                        ("●", ink())
                    } else {
                        ("─", Style::default().fg(FAINT))
                    };
                    put(buf, bx + k, y, 1, s, st);
                }
                put_right(buf, right - 1, y, vw, value, if selected { bold() } else { muted() });
                let br = rect(bx, y, bw, 1);
                ctx.hit(line, Hit::SettingRow(i));
                ctx.hit(br, Hit::SettingSlider(i));
                sliders.push((i, br));
            }
            SRow::Action(k, label, danger, note) => {
                let pending = confirm == Some(*k);
                let st = if *danger { Style::default().fg(ERROR) } else { ink() };
                let st = if selected { st.add_modifier(Modifier::BOLD) } else { st };
                let lw = put(buf, lx, y, w / 2, label, st);
                if pending {
                    put(buf, lx + lw + 2, y, w / 2, "press Enter again to confirm", Style::default().fg(ERROR));
                } else if let Some(n) = note {
                    put_right(buf, right - 1, y, w / 2, n, muted());
                }
                ctx.hit(line, Hit::SettingRow(i));
            }
            SRow::Info(label, value) => {
                put(buf, lx, y, 22, label, muted());
                put(buf, lx + 22, y, w.saturating_sub(26), value, ink());
            }
            SRow::Note(n) => {
                for (k, l) in wrap(n, w.saturating_sub(4) as usize).iter().enumerate() {
                    put(buf, lx, y + k as u16, w.saturating_sub(4), l, dim());
                }
            }
            SRow::EqGraph => draw_eq_graph(buf, spec, gains, eq_on, rect(lx, y + 1, w.saturating_sub(4), 7)),
        }
        y += h;
    }
}

/// The ten bands as columns: the live level behind, the gain as a marker.
fn draw_eq_graph(buf: &mut Buffer, spec: &[f32; BANDS], gains: &[f32; 10], on: bool, r: Rect) {
    if r.width < 30 || r.height < 5 {
        return;
    }
    let bars = r.height - 1;
    let cw = r.width / 10;
    let levels = columns(spec, 10);
    let mid = r.y + bars / 2;
    for (b, g) in gains.iter().enumerate() {
        let x = r.x + b as u16 * cw;
        let w = cw.saturating_sub(2).max(1);
        // Live level, faint, from the bottom.
        let lv = levels[b] * bars as f32 * 8.0;
        for row in 0..bars {
            let from_bottom = (bars - 1 - row) as f32;
            let e = (lv - from_bottom * 8.0).clamp(0.0, 8.0) as usize;
            if e > 0 {
                put(buf, x, r.y + row, w, &EIGHTHS[e].repeat(w as usize), Style::default().fg(Color::Rgb(52, 52, 52)));
            }
        }
        put(buf, x, mid, w, &"┈".repeat(w as usize), Style::default().fg(FAINT));
        let off = (-g / crate::settings::EQ_MAX_DB * (bars as f32 / 2.0)).round() as i32;
        let gy = (mid as i32 + off).clamp(r.y as i32, (r.y + bars - 1) as i32) as u16;
        let st = if on { Style::default().fg(ACCENT()) } else { Style::default().fg(MUTED) };
        put(buf, x, gy, w, &"━".repeat(w as usize), st);
        let hz = crate::settings::EQ_BANDS[b];
        let label = if hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz}") };
        put(buf, x, r.y + bars, cw, &label, dim());
    }
}

// ---------------------------------------------------------------- player bar

fn draw_player(f: &mut Frame, app: &mut App, area: Rect) {
    fill(f.buffer_mut(), area, BAR);
    let line: String = "─".repeat(area.width as usize);
    put(f.buffer_mut(), area.x, area.y, area.width, &line, Style::default().fg(Color::Rgb(36, 36, 36)));
    let y = area.y + 1;
    let w = area.width;

    // Left: art and title.
    let left_w = (w * 28 / 100).max(20);
    let art = rect(area.x + 1, y, 8, 4);
    match app.current.clone() {
        Some(song) => {
            draw_art(f, &mut app.art, song.cover.as_deref(), &song.album, art, false);
            app.hits.push((art, Hit::OpenNowPlaying));
            let tx = area.x + 11;
            let tw = left_w.saturating_sub(12);
            let buf = f.buffer_mut();
            put(buf, tx, y, tw, &song.title, bold());
            put(buf, tx, y + 1, tw, &song.artist, muted());
            if app.cfg.settings.bar_visualizer && !app.status.paused && app.status.playing {
                draw_viz(buf, &app.spectrum, Visualizer::Ribbon, rect(tx, y + 2, tw.min(28), 1));
            }
            let heart = if song.starred { ("♥", accent()) } else { ("♡", muted()) };
            put(buf, tx, y + 3, 1, heart.0, heart.1);
            app.hits.push((rect(tx, y + 3, 2, 1), Hit::LikeCurrent));
            let status = if app.status.loading {
                format!("{} buffering", spin_frame())
            } else if app.status.paused {
                "paused".to_string()
            } else if app.cfg.settings.offline_mode {
                "offline".to_string()
            } else {
                String::new()
            };
            put(buf, tx + 3, y + 3, tw.saturating_sub(3), &status, dim());
            app.hits.push((rect(tx, y, tw, 2), Hit::OpenNowPlaying));
        }
        None => {
            placeholder(f.buffer_mut(), "splynt", art, false);
            put(f.buffer_mut(), area.x + 11, y + 1, left_w.saturating_sub(12), "Nothing playing", dim());
        }
    }

    // Right: volume, queue, full screen.
    let right_w = 26u16.min(w / 4);
    let rx = area.right().saturating_sub(right_w + 1);
    {
        let in_queue = matches!(app.top().map(|v| &v.kind), Some(Kind::Queue { .. }));
        let in_player = matches!(app.top().map(|v| &v.kind), Some(Kind::NowPlaying));
        let buf = f.buffer_mut();
        put(buf, rx, y, 7, "≣ Queue", if in_queue { accent() } else { muted() });
        app.hits.push((rect(rx, y, 7, 1), Hit::OpenQueue));
        put(buf, rx + 9, y, 8, "⤢ Player", if in_player { accent() } else { muted() });
        app.hits.push((rect(rx + 9, y, 8, 1), Hit::OpenNowPlaying));
        let vw = right_w.saturating_sub(9).max(4);
        put(buf, rx, y + 2, 3, "vol", dim());
        let filled = ((app.volume * vw as f32).round() as u16).min(vw);
        let bar: String = "━".repeat(filled as usize);
        let rest: String = "─".repeat((vw - filled) as usize);
        put(buf, rx + 4, y + 2, filled, &bar, ink());
        put(buf, rx + 4 + filled, y + 2, vw - filled, &rest, Style::default().fg(FAINT));
        put(buf, rx + 5 + vw, y + 2, 4, &format!("{:>3}%", (app.volume * 100.0).round() as u32), dim());
        let vr = rect(rx + 4, y + 2, vw, 1);
        app.volume_rect = vr;
        app.hits.push((vr, Hit::Volume));
        if app.cfg.settings.eq_enabled {
            put(buf, rx, y + 3, 4, "EQ", Style::default().fg(ACCENT()));
        }
        if app.cfg.settings.crossfade > 0 {
            put(buf, rx + 4, y + 3, 12, &format!("crossfade {}s", app.cfg.settings.crossfade), dim());
        }
    }

    // Center: controls and progress.
    let cx0 = area.x + left_w + 2;
    let cw = rx.saturating_sub(cx0 + 2);
    if cw < 20 {
        return;
    }
    let shuffle_style = if app.shuffle { accent() } else { muted() };
    let repeat = match app.repeat {
        Repeat::Off => ("↻ ", muted()),
        Repeat::All => ("↻ ", accent()),
        Repeat::One => ("↻1", accent()),
    };
    let play = if app.status.paused || app.current.is_none() { " ▶ " } else { " ❚❚" };
    let controls: [(&str, Style, Hit); 5] = [
        ("⇄ ", shuffle_style, Hit::Shuffle),
        ("|◀", ink(), Hit::Prev),
        (play, Style::default().bg(INK).fg(Color::Black).add_modifier(Modifier::BOLD), Hit::Toggle),
        ("▶|", ink(), Hit::Next),
        (repeat.0, repeat.1, Hit::Repeat),
    ];
    let gap = 5u16;
    let total: u16 = controls.iter().map(|c| c.0.width() as u16).sum::<u16>() + gap * 4;
    let mut x = cx0 + cw.saturating_sub(total) / 2;
    for (label, style, hit) in controls {
        let lw = label.width() as u16;
        put(f.buffer_mut(), x, y, lw, label, style);
        app.hits.push((rect(x.saturating_sub(2), y, lw + 4, 1), hit));
        x += lw + gap;
    }

    let dur = app.duration();
    let pos = app.seek_preview.unwrap_or(app.status.position).min(if dur > 0.0 { dur } else { f64::MAX });
    let lt = fmt_time(pos);
    let rt = if dur > 0.0 { fmt_time(dur) } else { "--:--".into() };
    let by = y + 2;
    let bar_x = cx0 + 6;
    let bar_w = cw.saturating_sub(12);
    let buf = f.buffer_mut();
    put(buf, cx0, by, 5, &format!("{lt:>5}"), dim());
    put(buf, bar_x + bar_w + 1, by, 6, &rt, dim());
    let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let done = (frac * bar_w as f64).round() as u16;
    let buffered = if dur > 0.0 {
        ((app.status.buffered / dur).clamp(0.0, 1.0) * bar_w as f64).round() as u16
    } else {
        0
    };
    for i in 0..bar_w {
        let (s, st) = if i < done {
            ("━", Style::default().fg(ACCENT()))
        } else if i == done && app.current.is_some() {
            ("●", ink())
        } else if i < buffered {
            ("━", Style::default().fg(Color::Rgb(70, 70, 70)))
        } else {
            ("━", Style::default().fg(Color::Rgb(45, 45, 45)))
        };
        put(buf, bar_x + i, by, 1, s, st);
    }
    let sr = rect(bar_x, by, bar_w, 1);
    app.seek_rect = sr;
    app.hits.push((sr, Hit::Seek));
}

// ---------------------------------------------------------------- hints

fn draw_hints(f: &mut Frame, app: &App, area: Rect) {
    fill(f.buffer_mut(), area, BAR);
    let kind = app.top().map(|v| &v.kind);
    let keys: Vec<(&str, &str)> = match (&app.overlay, kind) {
        (Some(Overlay::Prompt(_)), _) => vec![("type", "a name"), ("enter", "save"), ("esc", "cancel")],
        (Some(_), _) => vec![("↑↓", "choose"), ("enter", "select"), ("esc", "close")],
        (None, Some(Kind::Search(s))) if s.editing => {
            vec![("type", "search"), ("↓ enter", "results"), ("esc", "clear, then leave the box"), ("ctrl+w", "delete word")]
        }
        (None, Some(Kind::Search(s))) if s.searched.is_empty() => {
            vec![("enter", "search again"), ("x", "forget"), ("esc", "type"), ("space", "play/pause")]
        }
        (None, Some(Kind::Search(_))) => vec![
            ("enter", "play/open"),
            ("←→↑↓", "move"),
            ("esc", "edit search"),
            ("m", "more"),
            ("a", "queue"),
            ("D", "download"),
            ("space", "play/pause"),
        ],
        (None, Some(Kind::Settings { .. })) => {
            let mut v = vec![("↑↓", "choose"), ("←→", "change"), ("enter", "open/toggle")];
            if app.stack.len() > 1 {
                v.push(("esc", "back"));
            }
            v.push(("space", "play/pause"));
            v
        }
        (None, Some(Kind::Queue { .. })) => vec![
            ("enter", "play"),
            ("shift+↑↓", "move"),
            ("x", "remove"),
            ("z", "shuffle"),
            ("c", "clear"),
            ("S", "save"),
            ("m", "more"),
        ],
        _ => {
            let mut v = vec![("enter", "open/play"), ("space", "play/pause"), ("n/p", "next/prev"), ("m", "more"), ("a", "queue"), ("D", "download")];
            if let Some(Kind::Grid { kind: GridKind::Albums(_), .. }) = kind {
                v.push(("o", "sort"));
            }
            v.push(("/", "search"));
            if app.stack.len() > 1 {
                v.push(("esc", "back"));
            }
            v.push(("1-9", "pages"));
            v.push(("?", "help"));
            v
        }
    };
    let buf = f.buffer_mut();
    let mut x = area.x + 1;
    for (k, label) in keys {
        let kt = format!(" {k} ");
        let need = kt.width() as u16 + label.width() as u16 + 2;
        if x + need > area.right() {
            break;
        }
        x += put(buf, x, area.y, kt.width() as u16, &kt, Style::default().bg(CHIP).fg(INK).add_modifier(Modifier::BOLD));
        x += 1;
        x += put(buf, x, area.y, label.width() as u16, label, muted());
        x += 2;
    }
}

// ---------------------------------------------------------------- overlays

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    rect(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn draw_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Overlay::Menu(m)) = &app.overlay else { return };
    let w = m
        .entries
        .iter()
        .map(|e| e.0.width())
        .chain(std::iter::once(m.title.width()))
        .max()
        .unwrap_or(10) as u16
        + 6;
    let w = w.clamp(24, 60);
    let max_rows = area.height.saturating_sub(6) as usize;
    let shown = m.entries.len().min(max_rows.max(3));
    let h = shown as u16 + 4;
    let r = match m.anchor {
        Some((x, y)) => {
            let x = x.min(area.right().saturating_sub(w + 1));
            let y = if y + h > area.bottom() { area.bottom().saturating_sub(h + 1) } else { y + 1 };
            rect(x, y, w, h)
        }
        None => centered(area, w, h),
    };
    app.hits.push((area, Hit::Outside));
    f.render_widget(Clear, r);
    let b = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FAINT))
        .style(Style::default().bg(Color::Rgb(40, 40, 40)));
    f.render_widget(b, r);
    let buf = f.buffer_mut();
    put(buf, r.x + 2, r.y + 1, w - 4, &m.title, dim().add_modifier(Modifier::BOLD));
    let start = (m.sel + 1).saturating_sub(shown);
    for (k, (i, (label, _))) in m.entries.iter().enumerate().skip(start).take(shown).enumerate() {
        let y = r.y + 2 + k as u16;
        let row = rect(r.x + 1, y, w - 2, 1);
        if i == m.sel {
            buf.set_style(row, Style::default().bg(Color::Rgb(62, 62, 62)));
        }
        put(buf, r.x + 2, y, w - 4, label, if i == m.sel { bold() } else { ink() });
        app.hits.push((row, Hit::MenuItem(i)));
    }
}

fn draw_prompt(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(Overlay::Prompt(p)) = &app.overlay else { return };
    let r = centered(area, 60, 7);
    app.hits.push((area, Hit::Outside));
    f.render_widget(Clear, r);
    let b = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT()))
        .style(Style::default().bg(Color::Rgb(32, 32, 32)));
    f.render_widget(b, r);
    let buf = f.buffer_mut();
    put(buf, r.x + 2, r.y + 1, r.width - 4, &p.title, bold());
    let field = rect(r.x + 2, r.y + 3, r.width - 4, 1);
    fill(buf, field, SEL);
    let w = put(buf, field.x + 1, field.y, field.width - 3, &p.text, ink());
    put(buf, field.x + 1 + w, field.y, 1, "▏", accent());
    put(buf, r.x + 2, r.y + 5, r.width - 4, "enter saves · esc cancels", dim());
}

fn draw_help(f: &mut Frame, app: &mut App, area: Rect) {
    let groups: [(&str, &[(&str, &str)]); 4] = [
        (
            "Moving around",
            &[
                ("↑ ↓ ← →  j k h", "move"),
                ("enter", "open, or play from here"),
                ("esc  backspace", "go back"),
                ("1 – 9", "sidebar pages"),
                ("tab  shift+tab", "next or previous page"),
                ("/", "search"),
                ("f", "full screen player"),
                (",", "settings"),
            ],
        ),
        (
            "Playback",
            &[
                ("space", "play or pause"),
                ("n  p", "next or previous song"),
                ("[  ]", "back or forward 10 seconds"),
                ("-  =", "volume down or up"),
                ("s  r", "shuffle, repeat"),
                ("L", "like the song that's playing"),
            ],
        ),
        (
            "On the selection",
            &[
                ("m", "more options, or right-click"),
                ("a  A", "add to queue, play next"),
                ("D", "download"),
                ("l", "like or unlike"),
                ("R", "start radio"),
                ("P  S", "play or shuffle this page"),
                ("o", "change the album sort"),
            ],
        ),
        (
            "Queue",
            &[
                ("shift+↑↓  J K", "move a song, or drag it"),
                ("x", "remove"),
                ("z  c", "shuffle or clear up next"),
                ("S", "save as a playlist"),
                ("q", "quit and save the queue"),
            ],
        ),
    ];
    let height = |gs: &[(&str, &[(&str, &str)])]| gs.iter().map(|g| g.1.len() + 2).sum::<usize>() as u16;
    let two = area.width >= 120;
    let (cols, col_w): (Vec<&[(&str, &[(&str, &str)])]>, u16) = if two {
        (vec![&groups[..2], &groups[2..]], 56)
    } else {
        (vec![&groups[..]], 76)
    };
    let h = cols.iter().map(|c| height(c)).max().unwrap_or(10) + 3;
    let r = centered(area, col_w * cols.len() as u16 + 4, h);
    app.hits.push((area, Hit::Outside));
    f.render_widget(Clear, r);
    let b = Block::bordered()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT()))
        .title(" Keys ")
        .style(Style::default().bg(Color::Rgb(28, 28, 28)));
    f.render_widget(b, r);
    let buf = f.buffer_mut();
    for (ci, col) in cols.iter().enumerate() {
        let x0 = r.x + 3 + ci as u16 * col_w;
        let mut y = r.y + 1;
        for (title, rows) in col.iter() {
            if y + 1 >= r.bottom().saturating_sub(1) {
                break;
            }
            put(buf, x0, y, col_w - 2, title, bold().fg(ACCENT()));
            y += 1;
            for (k, d) in rows.iter() {
                if y >= r.bottom().saturating_sub(2) {
                    break;
                }
                put(buf, x0 + 2, y, 16, k, bold());
                put(buf, x0 + 19, y, col_w.saturating_sub(21), d, muted());
                y += 1;
            }
            y += 1;
        }
    }
    let mode = app.art.as_ref().map(|a| a.protocol_name()).unwrap_or("off");
    put(buf, r.x + 3, r.bottom() - 2, r.width - 6, &format!("Cover art: {mode}. The mouse works everywhere. Any key closes this."), dim());
}

// ---------------------------------------------------------------- login

fn draw_login(f: &mut Frame, app: &mut App, area: Rect) {
    let Screen::Login(form) = &app.screen else { return };
    let r = centered(area, 62, 22);
    let buf = f.buffer_mut();
    let mut y = r.y;
    for l in LOGO {
        let w = l.width() as u16;
        put(buf, r.x + (r.width.saturating_sub(w)) / 2, y, w, l, accent());
        y += 1;
    }
    y += 1;
    let tag = "Your Navidrome library, in the terminal.";
    put(buf, r.x + (r.width.saturating_sub(tag.width() as u16)) / 2, y, r.width, tag, muted());
    y += 2;
    let labels = ["Server address", "Username", "Password"];
    let hints = ["music.example.com or http://192.168.1.10:4533", "", ""];
    let mut hits = Vec::new();
    for i in 0..3 {
        let active = form.field == i;
        put(f.buffer_mut(), r.x + 2, y, 30, labels[i], if active { bold() } else { muted() });
        y += 1;
        let field = rect(r.x + 2, y, r.width - 4, 1);
        fill(f.buffer_mut(), field, if active { SEL } else { CHIP });
        if active {
            put(f.buffer_mut(), r.x + 1, y, 1, "▌", accent());
        }
        let value = if i == 2 { "•".repeat(form.fields[i].chars().count()) } else { form.fields[i].clone() };
        let buf = f.buffer_mut();
        if value.is_empty() && !hints[i].is_empty() {
            put(buf, r.x + 4, y, r.width - 7, hints[i], dim());
        }
        let shown = if value.width() > (r.width - 7) as usize {
            let skip = value.chars().count() - (r.width - 8) as usize;
            format!("…{}", value.chars().skip(skip).collect::<String>())
        } else {
            value
        };
        let w = if shown.is_empty() { 0 } else { put(buf, r.x + 3, y, r.width - 6, &shown, ink()) };
        if active {
            put(buf, r.x + 3 + w, y, 1, "▏", accent());
        }
        hits.push((field, Hit::LoginField(i)));
        y += 2;
    }
    let label = if form.busy { format!(" {} Signing in… ", spin_frame()) } else { "  Sign in  ".into() };
    let bw = label.width() as u16;
    let bx = r.x + (r.width - bw) / 2;
    let style = if form.field == 3 {
        Style::default().bg(INK).fg(Color::Black).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(ACCENT()).fg(ACCENT_INK).add_modifier(Modifier::BOLD)
    };
    put(f.buffer_mut(), bx, y, bw, &label, style);
    hits.push((rect(bx, y, bw, 1), Hit::LoginSubmit));
    y += 2;
    if let Some(e) = &form.error {
        let w = e.width() as u16;
        put(f.buffer_mut(), r.x + (r.width.saturating_sub(w)) / 2, y, r.width, e, Style::default().fg(ERROR));
    }
    y += 2;
    let foot = "tab next field · enter continue · esc quit";
    put(f.buffer_mut(), r.x + (r.width.saturating_sub(foot.width() as u16)) / 2, y, r.width, foot, dim());
    let foot2 = "Your password is never saved; only a salted token is.";
    put(f.buffer_mut(), r.x + (r.width.saturating_sub(foot2.width() as u16)) / 2, y + 1, r.width, foot2, Style::default().fg(FAINT));
    app.hits.extend(hits);
}
