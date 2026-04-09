import { useCallback } from "react";
import { Box } from "@mui/material";
import type { SxProps, Theme } from "@mui/material/styles";

export interface ResizeHandleProps {
  direction: "horizontal" | "vertical";
  /** Incremental delta in pixels per pointer move (horizontal: X, vertical: Y). */
  onResize: (delta: number) => void;
  sx?: SxProps<Theme>;
}

/**
 * Draggable splitter — use between flex panels. Cursor and pointer capture follow direction.
 */
export function ResizeHandle({ direction, onResize, sx }: ResizeHandleProps) {
  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      const target = e.currentTarget;
      target.setPointerCapture(e.pointerId);

      let lastX = e.clientX;
      let lastY = e.clientY;

      const move = (ev: PointerEvent) => {
        if (direction === "horizontal") {
          const d = ev.clientX - lastX;
          lastX = ev.clientX;
          if (d !== 0) onResize(d);
        } else {
          const d = ev.clientY - lastY;
          lastY = ev.clientY;
          if (d !== 0) onResize(d);
        }
      };

      const up = (ev: PointerEvent) => {
        target.releasePointerCapture(ev.pointerId);
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", up);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.body.style.cursor =
        direction === "horizontal" ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [direction, onResize],
  );

  const isH = direction === "horizontal";

  return (
    <Box
      role="separator"
      aria-orientation={isH ? "vertical" : "horizontal"}
      aria-label={isH ? "左右调整分区宽度" : "上下调整分区高度"}
      onPointerDown={onPointerDown}
      sx={{
        flexShrink: 0,
        touchAction: "none",
        bgcolor: (theme) => theme.palette.divider,
        transition: "background-color 0.15s",
        "&:hover": {
          bgcolor: "primary.main",
        },
        ...(isH
          ? {
              width: 6,
              cursor: "col-resize",
              mx: -0.25,
              zIndex: 1,
            }
          : {
              height: 6,
              cursor: "row-resize",
              my: -0.25,
              zIndex: 1,
            }),
        ...sx,
      }}
    />
  );
}
