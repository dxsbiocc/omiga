import { type ReactNode } from "react";
import {
  Box,
  Drawer,
  IconButton,
  Stack,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import { Close as CloseIcon } from "@mui/icons-material";

export interface RightDetailDrawerProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  subtitle?: ReactNode;
  children: ReactNode;
  width?: number;
  closeLabel?: string;
  titleWeight?: number;
  titleAlign?: "center" | "flex-start";
}

export function RightDetailDrawer({
  open,
  onClose,
  title,
  subtitle,
  children,
  width = 440,
  closeLabel = "Close detail",
  titleWeight = 600,
  titleAlign = "center",
}: RightDetailDrawerProps) {
  const theme = useTheme();
  const paper = theme.palette.background.paper;
  const edge = alpha(
    theme.palette.mode === "dark"
      ? theme.palette.common.white
      : theme.palette.common.black,
    0.08,
  );

  return (
    <Drawer
      anchor="right"
      open={open}
      onClose={onClose}
      PaperProps={{
        sx: {
          width: { xs: "100%", sm: width },
          maxWidth: "100vw",
          bgcolor: alpha(paper, 0.98),
          borderLeft: `1px solid ${edge}`,
        },
      }}
    >
      <Stack
        direction="row"
        alignItems={titleAlign}
        justifyContent="space-between"
        sx={{
          px: 2,
          py: 1.5,
          borderBottom: `1px solid ${edge}`,
        }}
      >
        <Typography variant="subtitle1" fontWeight={titleWeight} sx={{ pr: 1 }}>
          {title}
          {subtitle ? (
            <Typography
              component="span"
              variant="body2"
              color="text.secondary"
              sx={{ display: "block", fontWeight: 500, mt: 0.25 }}
            >
              {subtitle}
            </Typography>
          ) : null}
        </Typography>
        <IconButton size="small" aria-label={closeLabel} onClick={onClose}>
          <CloseIcon />
        </IconButton>
      </Stack>

      <Box sx={{ flex: 1, overflow: "auto", p: 2 }}>{children}</Box>
    </Drawer>
  );
}
