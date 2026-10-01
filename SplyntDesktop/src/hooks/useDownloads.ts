import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DownloadFailure, DownloadItem, DownloadProgress, SongSummary } from "../types";

export type BatchResult = { completed: number; failed: SongSummary[] };

export function useDownloads() {
  const [items, setItems] = useState<DownloadItem[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [failures, setFailures] = useState<DownloadFailure[]>([]);
  const [loading, setLoading] = useState(true);
  const cancelBatch = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const value = await invoke<DownloadItem[]>("list_downloads");
      setItems(Array.isArray(value) ? value : []);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    let dispose: (() => void) | undefined;
    void listen<DownloadProgress>("download-progress", ({ payload }) => {
      setProgress((current) => ({ ...current, [payload.id]: payload }));
      if (payload.status === "complete") void refresh();
    }).then((unlisten) => { dispose = unlisten; }).catch(() => undefined);
    return () => dispose?.();
  }, [refresh]);

  const download = useCallback(async (song: SongSummary) => {
    setFailures((current) => current.filter((failure) => failure.song.id !== song.id));
    setProgress((current) => ({ ...current, [song.id]: { id: song.id, received: 0, status: "downloading" } }));
    try {
      const item = await invoke<DownloadItem>("download_song", { song });
      setItems((current) => [...current.filter((existing) => existing.song.id !== song.id), item]);
      return item;
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The download failed.";
      const paused = message.startsWith("Download paused");
      setProgress((current) => ({ ...current, [song.id]: { ...current[song.id], id: song.id, received: current[song.id]?.received ?? 0, status: paused ? "paused" : "failed", message } }));
      // Recorded rather than only thrown: a batch keeps going past one bad
      // track, and Downloads offers a retry for everything that did not land.
      setFailures((current) => [...current.filter((failure) => failure.song.id !== song.id), { song, message, paused }]);
      throw reason;
    }
  }, []);

  const downloadMany = useCallback(async (songs: SongSummary[]): Promise<BatchResult> => {
    cancelBatch.current = false;
    let completed = 0;
    const failed: SongSummary[] = [];
    for (const song of songs) {
      if (cancelBatch.current) {
        failed.push(song);
        continue;
      }
      try {
        await download(song);
        completed += 1;
      } catch {
        // One track pausing or failing never abandons the rest of the batch.
        failed.push(song);
      }
    }
    return { completed, failed };
  }, [download]);

  const stopBatch = useCallback(() => { cancelBatch.current = true; }, []);

  const retryFailed = useCallback(async () => {
    const pending = failures.map((failure) => failure.song);
    if (!pending.length) return { completed: 0, failed: [] } as BatchResult;
    return downloadMany(pending);
  }, [downloadMany, failures]);

  const pause = useCallback(async (id: string) => {
    await invoke("pause_download", { id });
  }, []);

  const remove = useCallback(async (id: string) => {
    await invoke("remove_download", { id });
    setItems((current) => current.filter((item) => item.song.id !== id));
    setFailures((current) => current.filter((failure) => failure.song.id !== id));
    setProgress((current) => {
      const next = { ...current };
      delete next[id];
      return next;
    });
  }, []);

  const clear = useCallback(async () => {
    cancelBatch.current = true;
    await invoke("clear_downloads");
    setItems([]);
    setProgress({});
    setFailures([]);
  }, []);

  return useMemo(() => ({
    items,
    progress,
    failures,
    loading,
    downloadedIds: new Set(items.map((item) => item.song.id)),
    totalBytes: items.reduce((total, item) => total + item.bytes, 0),
    refresh,
    download,
    downloadMany,
    stopBatch,
    retryFailed,
    pause,
    remove,
    clear,
  }), [clear, download, downloadMany, failures, items, loading, pause, progress, refresh, remove, retryFailed, stopBatch]);
}

export type DownloadsController = ReturnType<typeof useDownloads>;
