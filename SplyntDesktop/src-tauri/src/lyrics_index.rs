//! Lyrics phrase search, as on iOS: an account-scoped index of the lyric
//! text from the connected server and from LRCLIB, answering "which song
//! has these words?" without asking the server, which cannot search lyrics.
//!
//! The server's lyrics replace LRCLIB's, because the listener may have edited
//! them on purpose. LRCLIB's stay until then. Native ids only: `ext-` ids are
//! volatile and never key durable state (ADR-0002).
//!
//! The whole index lives in one JSON file per account and is searched in
//! memory. A library of tens of thousands of songs is a few megabytes of
//! text, which a scan answers in milliseconds, so it needs no database.

use crate::models::SongSummary;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LyricsOrigin {
    Server,
    Lrclib,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    song: SongSummary,
    origin: LyricsOrigin,
    text: String,
    /// `text` folded one character for one character, so an offset found
    /// in it is the same character offset in `text`.
    #[serde(skip, default)]
    folded: String,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct LyricsIndex {
    /// Songs the server has answered for, with lyrics or without, so a crawl
    /// resumes where it stopped.
    #[serde(default)]
    scanned: HashSet<String>,
    #[serde(default)]
    entries: HashMap<String, Entry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LyricsMatch {
    pub(crate) song: SongSummary,
    pub(crate) snippet: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LyricsIndexStatus {
    pub(crate) scanned: usize,
    pub(crate) searchable: usize,
    pub(crate) total: usize,
    pub(crate) running: bool,
}

#[derive(Default)]
pub(crate) struct LyricsIndexState {
    pub(crate) index: Mutex<Option<(PathBuf, LyricsIndex)>>,
    pub(crate) dirty: AtomicBool,
    pub(crate) last_save: Mutex<Option<Instant>>,
    pub(crate) crawling: AtomicBool,
    pub(crate) total: Mutex<usize>,
}

pub(crate) fn is_native(id: &str) -> bool {
    !id.starts_with("ext-")
}

/// Lowercase with common Latin accents removed, always one character out
/// for one character in.
fn fold_char(character: char) -> char {
    let lower = character.to_lowercase().next().unwrap_or(character);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ś' | 'š' => 's',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        '’' | '‘' => '\'',
        other => other,
    }
}

fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// At most ten words. Punctuation splits words, so "don't" is "don" and "t".
fn query_tokens(query: &str) -> Vec<String> {
    fold(query)
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .take(10)
        .map(str::to_string)
        .collect()
}

/// The byte offset of the first word in `folded` that starts with `token`.
fn word_start(folded: &str, token: &str) -> Option<usize> {
    folded
        .match_indices(token)
        .map(|(offset, _)| offset)
        .find(|offset| {
            folded[..*offset]
                .chars()
                .next_back()
                .is_none_or(|previous| !previous.is_alphanumeric())
        })
}

/// About a dozen words around the first match, lines joined with " / ".
fn snippet(text: &str, folded: &str, offset: usize) -> String {
    let at = folded[..offset].chars().count();
    let characters: Vec<char> = text.chars().collect();
    let mut start = at.saturating_sub(36);
    while start > 0 && !characters[start - 1].is_whitespace() {
        start += 1;
        if start >= at {
            break;
        }
    }
    let mut end = (at + 72).min(characters.len());
    while end < characters.len() && !characters[end].is_whitespace() {
        end += 1;
    }
    let body: String = characters[start..end]
        .iter()
        .collect::<String>()
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    format!(
        "{}{}{}",
        if start > 0 { "… " } else { "" },
        body,
        if end < characters.len() { " …" } else { "" }
    )
}

impl LyricsIndex {
    pub(crate) fn load(path: &Path) -> Self {
        let mut index = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LyricsIndex>(&bytes).ok())
            .unwrap_or_default();
        for entry in index.entries.values_mut() {
            entry.folded = fold(&entry.text);
        }
        index
    }

    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(self).unwrap_or_default())?;
        std::fs::rename(temporary, path)
    }

    /// Returns whether the index changed. A server row is never replaced by
    /// LRCLIB's.
    pub(crate) fn insert(
        &mut self,
        song: SongSummary,
        origin: LyricsOrigin,
        lines: &[String],
    ) -> bool {
        if !is_native(&song.id) {
            return false;
        }
        let text = lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return false;
        }
        if let Some(existing) = self.entries.get(&song.id) {
            if existing.origin == LyricsOrigin::Server && origin == LyricsOrigin::Lrclib {
                return false;
            }
            if existing.origin == origin && existing.text == text {
                return false;
            }
        }
        // Keep the richer summary a crawl stored over the thinner one the
        // player passes, which can lack artwork.
        let song = match self.entries.get(&song.id) {
            Some(existing) if song.cover_art.is_none() => existing.song.clone(),
            _ => song,
        };
        let folded = fold(&text);
        self.entries.insert(
            song.id.clone(),
            Entry {
                song,
                origin,
                text,
                folded,
            },
        );
        true
    }

    pub(crate) fn mark_scanned(&mut self, id: &str) {
        self.scanned.insert(id.to_string());
    }

    pub(crate) fn is_scanned(&self, id: &str) -> bool {
        self.scanned.contains(id)
    }

    /// A full rebuild asks the server again for every song. LRCLIB rows stay
    /// until the server answers with its own.
    pub(crate) fn forget_server_checks(&mut self) {
        self.scanned.clear();
    }

    pub(crate) fn counts(&self) -> (usize, usize) {
        (self.scanned.len(), self.entries.len())
    }

    /// Every word of the query must start a word in the lyrics, so "run
    /// through the" finds "running through these streets" while the listener
    /// is still typing. Songs holding the words as one phrase rank first.
    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<LyricsMatch> {
        let tokens = query_tokens(query);
        if tokens.is_empty() || limit == 0 {
            return Vec::new();
        }
        let phrase = tokens.join(" ");
        let mut hits: Vec<(bool, usize, &Entry)> = self
            .entries
            .values()
            .filter_map(|entry| {
                let first = word_start(&entry.folded, &tokens[0])?;
                if tokens[1..]
                    .iter()
                    .any(|token| word_start(&entry.folded, token).is_none())
                {
                    return None;
                }
                let exact = word_start(&entry.folded, &phrase);
                Some((exact.is_none(), exact.unwrap_or(first), entry))
            })
            .collect();
        hits.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.2.song.title.cmp(&b.2.song.title))
        });
        hits.into_iter()
            .take(limit)
            .map(|(_, offset, entry)| LyricsMatch {
                song: entry.song.clone(),
                snippet: snippet(&entry.text, &entry.folded, offset),
            })
            .collect()
    }
}
