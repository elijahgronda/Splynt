//! Search ranking, with the same rules as the iPhone and desktop apps.
//!
//! The library comes before providers, so an octo-fiesta row never sits above
//! the listener's own music. Inside each source, rows sort by how well they
//! match:
//!
//!   1. an exact name or artist
//!   2. a name or artist that starts with the text
//!   3. every typed word starts a word in the name, artist or album
//!   4. the text appears inside the name
//!   5. anything else the server matched
//!
//! In levels 1 and 2 a row matched by its own name beats one matched only by
//! its artist, so "drake" leads with the artist rather than five of his songs.
//! Ties keep the server's order, then songs, artists, albums, playlists.

use crate::api::{self, Album, Artist, Playlist, SearchResults, Song};
use crate::app::Item;

/// Lowercase, accents folded to ASCII where it's easy, punctuation dropped.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let c = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' | 'ń' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ý' | 'ÿ' => 'y',
            'ß' => 's',
            '&' => ' ',
            c => c,
        };
        if c.is_alphanumeric() || c.is_whitespace() {
            out.push(c);
        } else if c == '\'' || c == '’' || c == '.' {
            // "don't" matches "dont", "a.b" matches "ab".
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words(s: &str) -> Vec<&str> {
    s.split(' ').filter(|w| !w.is_empty()).collect()
}

/// Lower is better. The second value is 0 for a match on the row's own name.
fn level(q: &str, name: &str, artist: &str, album: &str) -> (u8, u8) {
    let n = normalize(name);
    let a = normalize(artist);
    if n == q {
        return (1, 0);
    }
    if !a.is_empty() && a == q {
        return (1, 1);
    }
    if n.starts_with(q) {
        return (2, 0);
    }
    if !a.is_empty() && a.starts_with(q) {
        return (2, 1);
    }
    let qw = words(q);
    let hay = format!("{n} {a} {}", normalize(album));
    let hw = words(&hay);
    if !qw.is_empty() && qw.iter().all(|w| hw.iter().any(|h| h.starts_with(w))) {
        return (3, 0);
    }
    if n.contains(q) {
        return (4, 0);
    }
    (5, 0)
}

fn kind_rank(i: &Item) -> u8 {
    match i {
        Item::Song(_) => 0,
        Item::Artist(_) => 1,
        Item::Album(_) => 2,
        Item::Playlist(_) => 3,
    }
}

type Key = (u8, u8, u8, u8, usize);

fn key(q: &str, item: &Item, order: usize) -> Key {
    let external = api::is_external(item.id()) as u8;
    let (lvl, by_artist) = match item {
        Item::Song(s) => level(q, &s.title, &s.artist, &s.album),
        Item::Album(a) => level(q, &a.name, &a.artist, ""),
        Item::Artist(a) => level(q, &a.name, "", ""),
        Item::Playlist(p) => level(q, &p.name, "", ""),
    };
    (external, lvl, by_artist, kind_rank(item), order)
}

pub struct Ranked {
    pub top: Option<Item>,
    pub songs: Vec<Song>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub playlists: Vec<Playlist>,
}

pub fn rank(query: &str, r: SearchResults, playlists: &[Playlist], hide_explicit: bool) -> Ranked {
    let q = normalize(query);
    let mut songs: Vec<(Key, Song)> = r
        .songs
        .into_iter()
        .filter(|s| !(hide_explicit && s.explicit))
        .enumerate()
        .map(|(i, s)| (key(&q, &Item::Song(s.clone()), i), s))
        .collect();
    let mut artists: Vec<(Key, Artist)> =
        r.artists.into_iter().enumerate().map(|(i, a)| (key(&q, &Item::Artist(a.clone()), i), a)).collect();
    let mut albums: Vec<(Key, Album)> =
        r.albums.into_iter().enumerate().map(|(i, a)| (key(&q, &Item::Album(a.clone()), i), a)).collect();
    // The server's search3 has no playlists, so match the listener's own here.
    let mut pls: Vec<(Key, Playlist)> = playlists
        .iter()
        .enumerate()
        .map(|(i, p)| (key(&q, &Item::Playlist(p.clone()), i), p.clone()))
        .filter(|(k, _)| k.1 <= 4)
        .collect();
    songs.sort_by_key(|x| x.0);
    artists.sort_by_key(|x| x.0);
    albums.sort_by_key(|x| x.0);
    pls.sort_by_key(|x| x.0);

    let mut best: Option<(Key, Item)> = None;
    let mut consider = |k: Key, item: Item| {
        // The top result has to actually match, not just be something the
        // server returned.
        if k.1 > 4 {
            return;
        }
        let k = (k.0, k.1, k.2, k.3, 0);
        if best.as_ref().is_none_or(|(b, _)| k < *b) {
            best = Some((k, item));
        }
    };
    if let Some((k, s)) = songs.first() {
        consider(*k, Item::Song(s.clone()));
    }
    if let Some((k, a)) = artists.first() {
        consider(*k, Item::Artist(a.clone()));
    }
    if let Some((k, a)) = albums.first() {
        consider(*k, Item::Album(a.clone()));
    }
    if let Some((k, p)) = pls.first() {
        consider(*k, Item::Playlist(p.clone()));
    }
    Ranked {
        top: best.map(|b| b.1),
        songs: songs.into_iter().map(|x| x.1).collect(),
        artists: artists.into_iter().map(|x| x.1).collect(),
        albums: albums.into_iter().map(|x| x.1).collect(),
        playlists: pls.into_iter().map(|x| x.1).collect(),
    }
}

