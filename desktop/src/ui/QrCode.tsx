import { useMemo } from "react";
import type { QrMatrix } from "../lib/types";

const QUIET = 4;

/** Renders the backend's QR matrix as one SVG path. Always dark-on-white so any phone camera can read it. */
export function QrCode({ matrix, size = 232, label }: { matrix: QrMatrix; size?: number; label: string }) {
  const d = useMemo(() => {
    const n = matrix.size;
    let path = "";
    for (let y = 0; y < n; y++) {
      let x = 0;
      while (x < n) {
        if (matrix.modules[y * n + x] === "1") {
          let run = 1;
          while (x + run < n && matrix.modules[y * n + x + run] === "1") run++;
          path += `M${x + QUIET} ${y + QUIET}h${run}v1h-${run}z`;
          x += run;
        } else x++;
      }
    }
    return path;
  }, [matrix]);
  const total = matrix.size + QUIET * 2;
  return (
    <svg
      role="img"
      aria-label={label}
      width={size}
      height={size}
      viewBox={`0 0 ${total} ${total}`}
      shapeRendering="crispEdges"
      className="rounded-2xl"
      style={{ background: "#ffffff" }}
    >
      <path d={d} fill="#0a0b0d" />
    </svg>
  );
}
