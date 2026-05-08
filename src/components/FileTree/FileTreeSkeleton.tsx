/**
 * FileTreeSkeleton — replaces the CircularProgress spinner in FileTree.
 *
 * Renders 6 skeleton rows that match the actual table column layout:
 *   col 1 (34px)  — checkbox placeholder
 *   col 2 (flex)  — icon chip (24×24) + name line
 *   col 3 (76px)  — file size
 *   col 4 (96px)  — modified date
 *
 * First 2 rows simulate folder entries (wider icon, lighter color);
 * next 4 simulate file entries (narrower names, shorter).
 * Stagger: baseDelay + rowIndex * 60 ms.
 */

import { Box, Table, TableBody, TableCell, TableRow } from "@mui/material";
import { ShimmerBox, ShimmerIconChip } from "../Skeletons/OmigaSkeleton";

interface SkeletonRowProps {
  type: "folder" | "file";
  nameWidth: string;
  baseDelay: number;
}

function SkeletonRow({ type, nameWidth, baseDelay }: SkeletonRowProps) {
  const iconRadius = type === "folder" ? 6 : 4;
  return (
    <TableRow sx={{ height: 36 }}>
      {/* Checkbox column */}
      <TableCell
        padding="none"
        sx={{ width: 34, maxWidth: 34, pl: 0.75, pr: 0.25, verticalAlign: "middle" }}
      >
        <ShimmerBox width={16} height={16} radius={3} delay={baseDelay} />
      </TableCell>

      {/* Name column */}
      <TableCell
        padding="none"
        sx={{ pl: 0.25, pr: 1, verticalAlign: "middle" }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
          <ShimmerIconChip size={24} radius={iconRadius} delay={baseDelay + 40} />
          <ShimmerBox width={nameWidth} height={13} radius={5} delay={baseDelay + 80} />
        </Box>
      </TableCell>

      {/* Size column */}
      <TableCell
        padding="none"
        sx={{ width: 76, maxWidth: 76, px: 0.5, verticalAlign: "middle" }}
      >
        {type === "file" ? (
          <ShimmerBox width="52%" height={11} radius={4} delay={baseDelay + 120} />
        ) : null}
      </TableCell>

      {/* Modified date column */}
      <TableCell
        padding="none"
        sx={{ width: 96, maxWidth: 96, pr: 1, verticalAlign: "middle" }}
      >
        <ShimmerBox width="75%" height={11} radius={4} delay={baseDelay + 160} />
      </TableCell>
    </TableRow>
  );
}

// ── Public component ──────────────────────────────────────────────────────

export function FileTreeSkeleton() {
  const rows: SkeletonRowProps[] = [
    { type: "folder", nameWidth: "55%", baseDelay: 0 },
    { type: "folder", nameWidth: "42%", baseDelay: 60 },
    { type: "file",   nameWidth: "68%", baseDelay: 120 },
    { type: "file",   nameWidth: "50%", baseDelay: 180 },
    { type: "file",   nameWidth: "74%", baseDelay: 240 },
    { type: "file",   nameWidth: "38%", baseDelay: 300 },
  ];

  return (
    <Table
      size="small"
      sx={{ width: "100%", borderCollapse: "separate", borderSpacing: 0 }}
    >
      <colgroup>
        <col style={{ width: 34 }} />
        <col style={{ minWidth: 160 }} />
        <col style={{ width: 76 }} />
        <col style={{ width: 96 }} />
      </colgroup>
      <TableBody>
        {rows.map((row, i) => (
          <SkeletonRow key={i} {...row} />
        ))}
      </TableBody>
    </Table>
  );
}
