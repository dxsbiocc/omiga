import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
    host: true,
  },
  build: {
    // Desktop apps legitimately ship a few large lazy vendor chunks (Monaco,
    // Plotly, pdf.js). Keep warnings focused on unexpected growth in the app
    // shell instead of known heavy optional viewers.
    chunkSizeWarningLimit: 5000,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;

          if (id.includes("plotly.js-dist-min")) return "vendor-plotly";
          if (
            id.includes("monaco-editor") ||
            id.includes("@monaco-editor")
          ) {
            return "vendor-monaco";
          }
          if (id.includes("pdfjs-dist") || id.includes("react-pdf")) {
            return "vendor-pdf";
          }
          if (id.includes("echarts")) return "vendor-echarts";
          if (
            id.includes("katex") ||
            id.includes("remark-math") ||
            id.includes("rehype-katex")
          ) {
            return "vendor-markdown-math";
          }
          if (id.includes("@mui") || id.includes("@emotion")) {
            return "vendor-mui";
          }
          if (id.includes("@xyflow") || id.includes("@dagrejs")) {
            return "vendor-flow";
          }
          return undefined;
        },
      },
    },
  },
}));
