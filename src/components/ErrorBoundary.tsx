import { Component, type ErrorInfo, type ReactNode } from "react";
import { Box, Button, Paper, Typography } from "@mui/material";

interface Props {
  children: ReactNode;
  /** Shown in the error panel title */
  label?: string;
}

interface State {
  error: Error | null;
  info: ErrorInfo | null;
}

const isDev = import.meta.env.DEV;

/**
 * Catches React render/lifecycle errors so the whole window does not go blank.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[ErrorBoundary${this.props.label ? `:${this.props.label}` : ""}]`, error, info);
    this.setState({ info });
  }

  handleReset = () => {
    this.setState({ error: null, info: null });
  };

  render() {
    const { error, info } = this.state;
    const { children, label } = this.props;

    if (error) {
      const stack = error.stack ?? String(error);
      const componentStack = info?.componentStack ?? "";

      return (
        <Paper
          elevation={0}
          sx={{
            m: 2,
            p: 2,
            border: 2,
            borderColor: "error.main",
            bgcolor: "error.light",
            maxHeight: "min(90vh, 560px)",
            overflow: "auto",
          }}
        >
          <Typography variant="subtitle1" fontWeight={700} color="error.dark" gutterBottom>
            {label ? `${label} — ` : ""}UI error (caught)
          </Typography>
          <Typography variant="body2" sx={{ mb: 1, fontFamily: "monospace", whiteSpace: "pre-wrap" }}>
            {error.message}
          </Typography>
          {isDev && (
            <>
              <Typography variant="caption" fontWeight={600} display="block" sx={{ mt: 1 }}>
                Stack
              </Typography>
              <Box
                component="pre"
                sx={{
                  fontSize: 11,
                  p: 1,
                  bgcolor: "background.paper",
                  borderRadius: 1,
                  overflow: "auto",
                  maxHeight: 200,
                }}
              >
                {stack}
              </Box>
              {componentStack ? (
                <>
                  <Typography variant="caption" fontWeight={600} display="block" sx={{ mt: 1 }}>
                    Component stack
                  </Typography>
                  <Box
                    component="pre"
                    sx={{
                      fontSize: 11,
                      p: 1,
                      bgcolor: "background.paper",
                      borderRadius: 1,
                      overflow: "auto",
                      maxHeight: 160,
                    }}
                  >
                    {componentStack}
                  </Box>
                </>
              ) : null}
            </>
          )}
          <Button variant="contained" color="inherit" size="small" sx={{ mt: 2 }} onClick={this.handleReset}>
            Try again
          </Button>
        </Paper>
      );
    }

    return children;
  }
}
