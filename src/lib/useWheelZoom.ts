import { useEffect, type RefObject } from "react";

interface UseWheelZoomOptions {
  min: number;
  max: number;
  /** Called with `"actual"` when a pinch/zoom gesture starts, so fit-mode can be exited. */
  onZoomStart?: () => void;
}

/**
 * Attach a non-passive wheel listener to `ref` that drives zoom via
 * Ctrl/⌘ + scroll or trackpad pinch.
 *
 * Why exponential (`Math.pow`) instead of a fixed step:
 *   - Trackpad pinch fires many tiny wheel events (deltaY ≈ 1–5 px each).
 *   - Mouse wheel fires few large events (deltaY ≈ 100 px each).
 *   - A fixed step would make the trackpad feel sluggish and the mouse feel jumpy.
 *   - `factor = 0.998 ^ deltaY` is proportional: small delta → tiny zoom change,
 *     large delta → bigger change, and it's symmetric (zoom-in / zoom-out cancel out).
 *
 * deltaMode normalisation:
 *   - mode 0 = pixels  (trackpad — values already in pixels, use as-is)
 *   - mode 1 = lines   (some mice — multiply by ~15 to get pixel-equivalent)
 *   - mode 2 = pages   (rare — multiply by ~300)
 */
export function useWheelZoom(
  ref: RefObject<HTMLElement | null>,
  setZoom: (updater: (prev: number) => number) => void,
  options: UseWheelZoomOptions,
) {
  const { min, max, onZoomStart } = options;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const handler = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return; // plain scroll → let browser pan
      e.preventDefault();

      // Normalise to pixel-equivalent delta
      let delta = e.deltaY;
      if (e.deltaMode === 1) delta *= 15;  // lines → px
      if (e.deltaMode === 2) delta *= 300; // pages → px

      onZoomStart?.();

      setZoom((prev) => {
        // Exponential: zoom ×= 0.998^delta
        // ≈ −5 px (pinch-out)  → ×1.01  (+1 %)
        // ≈ +5 px (pinch-in)   → ×0.99  (−1 %)
        // ≈ −100 px (wheel up) → ×1.22  (+22 %)
        const factor = Math.pow(0.998, delta);
        return Math.min(max, Math.max(min, Math.round(prev * factor)));
      });
    };

    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, [ref, setZoom, min, max, onZoomStart]);
}
