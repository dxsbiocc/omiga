import type { ReactNode } from "react";
import {
  Box,
  IconButton,
  Snackbar,
  Typography,
  type SnackbarProps,
} from "@mui/material";
import {
  CheckCircleOutline,
  CloseRounded,
  ErrorOutlineRounded,
  InfoOutlined,
  WarningAmberRounded,
} from "@mui/icons-material";
import {
  alpha,
  darken,
  lighten,
  useTheme,
} from "@mui/material/styles";

type NotificationSeverity = "success" | "info" | "warning" | "error";

interface NotificationToastProps {
  open: boolean;
  message: ReactNode;
  onClose: () => void;
  severity?: NotificationSeverity;
  title?: ReactNode;
  actions?: ReactNode;
  autoHideDuration?: SnackbarProps["autoHideDuration"];
  anchorOrigin?: SnackbarProps["anchorOrigin"];
  zIndexOffset?: number;
}

const TOAST_TITLES: Record<NotificationSeverity, string> = {
  success: "操作成功",
  info: "消息通知",
  warning: "需要注意",
  error: "操作失败",
};

const TOAST_ICONS = {
  success: CheckCircleOutline,
  info: InfoOutlined,
  warning: WarningAmberRounded,
  error: ErrorOutlineRounded,
} satisfies Record<NotificationSeverity, typeof CheckCircleOutline>;

const WAVE_PATH =
  "M0,256L11.4,240C22.9,224,46,192,69,192C91.4,192,114,224,137,234.7C160,245,183,235,206,213.3C228.6,192,251,160,274,149.3C297.1,139,320,149,343,181.3C365.7,213,389,267,411,282.7C434.3,299,457,277,480,250.7C502.9,224,526,192,549,181.3C571.4,171,594,181,617,208C640,235,663,277,686,256C708.6,235,731,149,754,122.7C777.1,96,800,128,823,165.3C845.7,203,869,245,891,224C914.3,203,937,117,960,112C982.9,107,1006,181,1029,197.3C1051.4,213,1074,171,1097,144C1120,117,1143,107,1166,133.3C1188.6,160,1211,224,1234,218.7C1257.1,213,1280,139,1303,133.3C1325.7,128,1349,192,1371,192C1394.3,192,1417,128,1429,96L1440,64L1440,320L1428.6,320C1417.1,320,1394,320,1371,320C1348.6,320,1326,320,1303,320C1280,320,1257,320,1234,320C1211.4,320,1189,320,1166,320C1142.9,320,1120,320,1097,320C1074.3,320,1051,320,1029,320C1005.7,320,983,320,960,320C937.1,320,914,320,891,320C868.6,320,846,320,823,320C800,320,777,320,754,320C731.4,320,709,320,686,320C662.9,320,640,320,617,320C594.3,320,571,320,549,320C525.7,320,503,320,480,320C457.1,320,434,320,411,320C388.6,320,366,320,343,320C320,320,297,320,274,320C251.4,320,229,320,206,320C182.9,320,160,320,137,320C114.3,320,91,320,69,320C45.7,320,23,320,11,320L0,320Z";

export function NotificationToast({
  open,
  message,
  onClose,
  severity = "info",
  title,
  actions,
  autoHideDuration = 4000,
  anchorOrigin,
  zIndexOffset = 0,
}: NotificationToastProps) {
  const theme = useTheme();
  const resolvedAnchorOrigin = anchorOrigin ?? {
    vertical: "top" as const,
    horizontal: "center" as const,
  };
  const isDark = theme.palette.mode === "dark";
  const accent = theme.palette[severity].main;
  const accentStrong = isDark ? lighten(accent, 0.22) : darken(accent, 0.08);
  const Icon = TOAST_ICONS[severity];
  const offsetSx =
    resolvedAnchorOrigin.vertical === "top"
      ? {
          top: {
            xs: "calc(env(safe-area-inset-top) + 12px)",
            sm: "calc(env(safe-area-inset-top) + 20px)",
          },
        }
      : {
          bottom: {
            xs: "calc(env(safe-area-inset-bottom) + 12px)",
            sm: "calc(env(safe-area-inset-bottom) + 20px)",
          },
        };

  const handleClose: SnackbarProps["onClose"] = (_event, reason) => {
    if (reason === "clickaway") return;
    onClose();
  };

  return (
    <Snackbar
      open={open}
      autoHideDuration={autoHideDuration}
      onClose={handleClose}
      anchorOrigin={resolvedAnchorOrigin}
      sx={{
        ...offsetSx,
        zIndex: (t) => t.zIndex.snackbar + zIndexOffset,
      }}
    >
      <Box
        role={severity === "error" ? "alert" : "status"}
        aria-live={severity === "error" ? "assertive" : "polite"}
        sx={{
          "@keyframes toastSlideDown": {
            from: { opacity: 0, transform: "translate3d(0, -12px, 0) scale(0.98)" },
            to: { opacity: 1, transform: "translate3d(0, 0, 0) scale(1)" },
          },
          position: "relative",
          display: "flex",
          alignItems: "center",
          gap: 1.5,
          width: { xs: "calc(100vw - 32px)", sm: 360 },
          minHeight: 82,
          boxSizing: "border-box",
          px: 1.75,
          py: 1.4,
          overflow: "hidden",
          borderRadius: 3,
          bgcolor: isDark
            ? alpha(theme.palette.background.paper, 0.94)
            : alpha(theme.palette.background.paper, 0.98),
          border: `1px solid ${alpha(accent, isDark ? 0.32 : 0.16)}`,
          boxShadow: isDark
            ? `0 18px 48px ${alpha("#000", 0.42)}, 0 0 0 1px ${alpha(accent, 0.12)}`
            : `0 18px 46px ${alpha(accent, 0.16)}, 0 12px 28px ${alpha("#0f172a", 0.12)}`,
          backdropFilter: "blur(18px)",
          animation: "toastSlideDown 220ms cubic-bezier(0.2, 0.9, 0.2, 1)",
        }}
      >
        <Box
          component="svg"
          aria-hidden="true"
          viewBox="0 0 1440 320"
          sx={{
            position: "absolute",
            left: -39,
            top: 24,
            width: 92,
            color: alpha(accent, isDark ? 0.22 : 0.17),
            transform: "rotate(90deg)",
            pointerEvents: "none",
          }}
        >
          <path d={WAVE_PATH} fill="currentColor" />
        </Box>

        <Box
          sx={{
            zIndex: 1,
            width: 42,
            height: 42,
            flex: "0 0 auto",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            ml: 0.5,
            borderRadius: "50%",
            color: accentStrong,
            bgcolor: alpha(accent, isDark ? 0.18 : 0.13),
            boxShadow: `inset 0 0 0 1px ${alpha(accent, isDark ? 0.18 : 0.12)}`,
          }}
        >
          <Icon sx={{ fontSize: 22 }} />
        </Box>

        <Box
          sx={{
            zIndex: 1,
            minWidth: 0,
            flex: 1,
            display: "flex",
            flexDirection: "column",
            gap: 0.25,
          }}
        >
          <Typography
            variant="subtitle2"
            sx={{
              color: accentStrong,
              fontSize: 15.5,
              lineHeight: 1.25,
              fontWeight: 800,
              letterSpacing: "-0.01em",
            }}
            noWrap
          >
            {title ?? TOAST_TITLES[severity]}
          </Typography>
          <Typography
            variant="body2"
            sx={{
              color: "text.secondary",
              fontSize: 13,
              lineHeight: 1.45,
              display: "-webkit-box",
              WebkitBoxOrient: "vertical",
              WebkitLineClamp: 2,
              overflow: "hidden",
            }}
          >
            {message}
          </Typography>
          {actions ? (
            <Box
              sx={{
                mt: 0.8,
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: 0.75,
              }}
            >
              {actions}
            </Box>
          ) : null}
        </Box>

        <IconButton
          aria-label="关闭通知"
          size="small"
          onClick={onClose}
          sx={{
            zIndex: 1,
            width: 34,
            height: 34,
            flex: "0 0 auto",
            color: alpha(theme.palette.text.secondary, 0.82),
            "&:hover": {
              color: accentStrong,
              bgcolor: alpha(accent, isDark ? 0.16 : 0.08),
            },
            "&:focus-visible": {
              outline: `2px solid ${alpha(accent, 0.48)}`,
              outlineOffset: 2,
            },
          }}
        >
          <CloseRounded fontSize="small" />
        </IconButton>
      </Box>
    </Snackbar>
  );
}
