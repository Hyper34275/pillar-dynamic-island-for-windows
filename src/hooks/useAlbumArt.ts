import { useEffect, useState } from "react";
import { tauriInvoke } from "../lib/tauri";
import type { MediaInfo } from "./useMediaSession";

export interface AlbumArt {
  /** data: URL of the current track's artwork, or null when unavailable. */
  url: string | null;
  /** Vibrant color sampled from the artwork (#rrggbb), or null. */
  color: string | null;
}

const EMPTY: AlbumArt = { url: null, color: null };

/**
 * Pick a vibrant representative color from decoded artwork pixels. Pixels are
 * weighted by saturation so a colorful cover beats its grey background, and
 * near-black / near-white pixels are ignored.
 */
function sampleVibrantColor(url: string): Promise<string | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        const size = 24;
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) return resolve(null);
        ctx.drawImage(img, 0, 0, size, size);
        const { data } = ctx.getImageData(0, 0, size, size);
        let r = 0, g = 0, b = 0, total = 0;
        for (let i = 0; i < data.length; i += 4) {
          const pr = data[i], pg = data[i + 1], pb = data[i + 2];
          const max = Math.max(pr, pg, pb);
          const min = Math.min(pr, pg, pb);
          if (max < 40 || min > 225) continue;
          const sat = max === 0 ? 0 : (max - min) / max;
          const w = 0.15 + sat * sat * 4;
          r += pr * w; g += pg * w; b += pb * w; total += w;
        }
        if (total === 0) return resolve(null);
        r /= total; g /= total; b /= total;
        // Lift dark results so the accent stays legible on a black island.
        const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
        if (lum < 0.45) {
          const k = 0.45 / Math.max(lum, 0.05);
          const lift = Math.min(k, 2.6);
          r = Math.min(255, r * lift); g = Math.min(255, g * lift); b = Math.min(255, b * lift);
        }
        const hex = (v: number) => Math.round(v).toString(16).padStart(2, "0");
        resolve(`#${hex(r)}${hex(g)}${hex(b)}`);
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

/** Fetch artwork for the current track; refetches only when the track changes. */
export function useAlbumArt(media: MediaInfo | null): AlbumArt {
  const [art, setArt] = useState<AlbumArt>(EMPTY);
  const trackKey = media ? `${media.appName ?? ""}|${media.title}|${media.artist}` : null;

  useEffect(() => {
    if (!trackKey) {
      setArt(EMPTY);
      return;
    }
    let cancelled = false;
    let retryId: ReturnType<typeof setTimeout> | null = null;
    let currentUrl: string | null = null;
    // Drop the previous track's cover immediately rather than showing it for the
    // new title while the fetch (and any retries) are in flight.
    setArt(EMPTY);

    const load = async (attempt: number) => {
      let url: string | null = null;
      try {
        url = await tauriInvoke<string | null>("get_media_thumbnail", undefined, { silent: true });
      } catch {
        url = null;
      }
      if (cancelled) return;
      // Players often publish the thumbnail a beat after the title changes — so a
      // first fetch can return nothing or even the previous cover. Retry while empty,
      // and always re-check once so a stale cover gets replaced.
      if (attempt < 2) retryId = setTimeout(() => void load(attempt + 1), 1200);
      if (!url || url === currentUrl) return;
      currentUrl = url;
      const color = await sampleVibrantColor(url);
      if (!cancelled && currentUrl === url) setArt({ url, color });
    };

    void load(0);
    return () => {
      cancelled = true;
      if (retryId) clearTimeout(retryId);
    };
  }, [trackKey]);

  return art;
}
