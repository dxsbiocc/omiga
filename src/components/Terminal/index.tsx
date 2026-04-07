import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  TextField,
  Typography,
  IconButton,
  useTheme,
  alpha,
} from "@mui/material";
import {
  Send,
  Clear,
  Terminal as TerminalIcon,
} from "@mui/icons-material";

interface TerminalOutput {
  type: "input" | "output" | "error";
  content: string;
  timestamp: number;
}

interface TerminalProps {
  /** Hide the built-in title bar when embedded inside another tabbed shell (e.g. Chat). */
  embedded?: boolean;
}

export function Terminal({ embedded = false }: TerminalProps) {
  const theme = useTheme();
  const [history, setHistory] = useState<TerminalOutput[]>([]);
  const [input, setInput] = useState("");
  const [isExecuting, setIsExecuting] = useState(false);
  const outputRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    outputRef.current?.scrollIntoView({ behavior: "auto" });
  }, [history]);

  const handleExecute = async () => {
    if (!input.trim() || isExecuting) return;

    const command = input.trim();
    
    // Add input to history
    setHistory((prev) => [
      ...prev,
      { type: "input", content: `> ${command}`, timestamp: Date.now() },
    ]);
    
    setInput("");
    setIsExecuting(true);

    try {
      // Execute the command via Rust
      const result = await invoke<{ output: string; error?: string }>(
        "execute_tool",
        {
          toolName: "bash",
          args: JSON.stringify({ command }),
        }
      );

      if (result.error) {
        setHistory((prev) => [
          ...prev,
          { type: "error", content: result.error!, timestamp: Date.now() },
        ]);
      } else {
        setHistory((prev) => [
          ...prev,
          { type: "output", content: result.output, timestamp: Date.now() },
        ]);
      }
    } catch (error) {
      setHistory((prev) => [
        ...prev,
        { type: "error", content: String(error), timestamp: Date.now() },
      ]);
    } finally {
      setIsExecuting(false);
    }
  };

  const handleClear = () => {
    setHistory([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "Enter" || e.shiftKey) return;
    const ne = e.nativeEvent;
    if (ne.isComposing || ne.keyCode === 229) return;
    e.preventDefault();
    handleExecute();
  };

  return (
    <Box sx={{ height: "100%", display: "flex", flexDirection: "column", bgcolor: "background.default" }}>
      {!embedded && (
        <Box
          sx={{
            px: 2,
            py: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            borderBottom: 1,
            borderColor: "divider",
            bgcolor: alpha(theme.palette.background.paper, 0.8),
          }}
        >
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <TerminalIcon fontSize="small" color="primary" />
            <Typography variant="body2" fontWeight={500}>
              Terminal
            </Typography>
          </Box>
          <IconButton size="small" onClick={handleClear}>
            <Clear fontSize="small" />
          </IconButton>
        </Box>
      )}
      {embedded && (
        <Box
          sx={{
            px: 1,
            py: 0.5,
            display: "flex",
            justifyContent: "flex-end",
            borderBottom: 1,
            borderColor: "divider",
            bgcolor: alpha(theme.palette.background.paper, 0.5),
          }}
        >
          <IconButton size="small" onClick={handleClear} aria-label="Clear terminal">
            <Clear fontSize="small" />
          </IconButton>
        </Box>
      )}

      {/* Output Area */}
      <Box
        sx={{
          flex: 1,
          overflow: "auto",
          p: 2,
          fontFamily: "JetBrains Mono, Monaco, Consolas, monospace",
          fontSize: "0.8125rem",
          lineHeight: 1.6,
        }}
      >
        {history.length === 0 ? (
          <Typography
            variant="body2"
            color="text.secondary"
            sx={{ textAlign: "center", mt: 4 }}
          >
            Type a command and press Enter to execute
          </Typography>
        ) : (
          history.map((item, index) => (
            <Box
              key={index}
              sx={{
                mb: 1,
                color:
                  item.type === "input"
                    ? "primary.main"
                    : item.type === "error"
                    ? "error.main"
                    : "text.primary",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {item.content}
            </Box>
          ))
        )}
        <div ref={outputRef} />
      </Box>

      {/* Input Area */}
      <Box
        sx={{
          p: 2,
          borderTop: 1,
          borderColor: "divider",
          bgcolor: alpha(theme.palette.background.paper, 0.8),
        }}
      >
        <TextField
          fullWidth
          placeholder="Enter command..."
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={isExecuting}
          InputProps={{
            startAdornment: (
              <Typography
                component="span"
                sx={{
                  mr: 1,
                  color: "success.main",
                  fontFamily: "JetBrains Mono, monospace",
                  fontWeight: 500,
                }}
              >
                $
              </Typography>
            ),
            endAdornment: (
              <IconButton
                size="small"
                onClick={handleExecute}
                disabled={isExecuting || !input.trim()}
                color="primary"
              >
                <Send fontSize="small" />
              </IconButton>
            ),
          }}
          sx={{
            "& .MuiOutlinedInput-root": {
              borderRadius: 2,
              fontFamily: "JetBrains Mono, monospace",
              fontSize: "0.875rem",
              bgcolor: alpha(theme.palette.background.paper, 0.8),
            },
          }}
        />
      </Box>
    </Box>
  );
}
