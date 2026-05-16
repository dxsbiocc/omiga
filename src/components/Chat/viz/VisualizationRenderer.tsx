import { useEffect, useRef, useState } from "react";
import { Box, Alert } from "@mui/material";
import katex from "katex";
import { MermaidFlow } from "./MermaidFlow";
import { DotFlow } from "./DotFlow";

export interface VizConfig {
  type: string;
  [key: string]: unknown;
}

function EChartView({ option }: { option: unknown }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let chart: { dispose: () => void; resize: () => void } | null = null;
    let handleResize: (() => void) | null = null;
    (async () => {
      try {
        // @ts-ignore - optional peer dependency
        const echarts = await import("echarts");
        if (disposed || !containerRef.current) return;
        const inst = echarts.init(containerRef.current);
        chart = inst;
        inst.setOption(option as never);
        handleResize = () => inst.resize();
        window.addEventListener("resize", handleResize);
      } catch {
        setError("ECharts 未安装，请运行 bun add echarts");
      }
    })();
    return () => {
      disposed = true;
      if (handleResize) window.removeEventListener("resize", handleResize);
      if (chart && chart.dispose) chart.dispose();
    };
  }, [option]);

  if (error) {
    return (
      <Alert severity="warning" sx={{ my: 1 }}>
        {error}
      </Alert>
    );
  }

  return <Box ref={containerRef} sx={{ width: "100%", height: 320 }} />;
}


function IframeView({ src }: { src: string }) {
  return (
    <Box
      component="iframe"
      src={src}
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      loading="lazy"
      sx={{
        width: "100%",
        height: 400,
        border: "none",
        borderRadius: 1,
        display: "block",
      }}
    />
  );
}

function HtmlSandbox({ html }: { html: string }) {
  const [blobUrl, setBlobUrl] = useState<string | null>(null);

  useEffect(() => {
    const blob = new Blob([html], { type: "text/html" });
    const url = URL.createObjectURL(blob);
    setBlobUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [html]);

  if (!blobUrl) return null;
  return <IframeView src={blobUrl} />;
}

function buildMolStarPdbUrl(pdbUrl: string) {
  const html = `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Mol* Viewer</title>
<style>
html,body{margin:0;padding:0;width:100%;height:100%;overflow:hidden;background:#000}
#app{width:100%;height:100%}
</style>
</head>
<body>
<div id="app"></div>
<script type="text/javascript" src="https://cdn.jsdelivr.net/npm/pdbe-molstar@3.3.0/build/pdbe-molstar-plugin.js"></script>
<script>
(function(){
  var viewer = new PDBeMolstarPlugin();
  viewer.render(document.getElementById('app'), {
    customData: { url: ${JSON.stringify(pdbUrl)}, format: 'pdb', binary: false },
    bgColor: { r: 0, g: 0, b: 0 },
    hideControls: false,
    subscribeEvents: false
  });
})();
</script>
</body>
</html>`;
  const blob = new Blob([html], { type: "text/html" });
  return URL.createObjectURL(blob);
}

function PdbView({ url }: { url: string }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    const blobUrl = buildMolStarPdbUrl(url);
    setSrc(blobUrl);
    return () => URL.revokeObjectURL(blobUrl);
  }, [url]);

  if (!src) return null;
  return <IframeView src={src} />;
}

function PlotlyView({ data, layout }: { data: unknown; layout?: unknown }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let chart: { react: (el: HTMLDivElement, data: unknown, layout: unknown) => void; purge: (el: HTMLDivElement) => void } | null = null;
    let handleResize: (() => void) | null = null;
    (async () => {
      try {
        // @ts-ignore - optional peer dependency
        const Plotly = await import("plotly.js-dist-min");
        if (disposed || !containerRef.current) return;
        chart = Plotly;
        Plotly.react(
          containerRef.current,
          (Array.isArray(data) ? data : [data]) as never,
          (layout || {}) as never,
        );
        handleResize = () => Plotly.Plots.resize(containerRef.current);
        window.addEventListener("resize", handleResize);
      } catch {
        setError("Plotly 未安装，请运行 bun add plotly.js-dist-min");
      }
    })();
    return () => {
      disposed = true;
      if (handleResize) window.removeEventListener("resize", handleResize);
      if (containerRef.current && chart) {
        try {
          chart.purge(containerRef.current);
        } catch {}
      }
    };
  }, [data, layout]);

  if (error) {
    return (
      <Alert severity="warning" sx={{ my: 1 }}>
        {error}
      </Alert>
    );
  }

  return <Box ref={containerRef} sx={{ width: "100%", height: 360 }} />;
}


function buildThreeIframe(code: string) {
  const html = `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Three.js</title>
<style>
html,body{margin:0;padding:0;width:100%;height:100%;overflow:hidden;background:#111}
</style>
</head>
<body>
<script src="https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.min.js"></script>
<script>
(function(THREE){
  ${code}
})(THREE);
</script>
</body>
</html>`;
  const blob = new Blob([html], { type: "text/html" });
  return URL.createObjectURL(blob);
}

function ThreeView({ code }: { code: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    const url = buildThreeIframe(code);
    setSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [code]);
  if (!src) return null;
  return <IframeView src={src} />;
}

function buildMapIframe(config: {
  center?: [number, number];
  zoom?: number;
  markers?: Array<{ lat: number; lng: number; popup?: string }>;
  geojson?: unknown;
}) {
  const rawCenter = config.center || [39.9042, 116.4074];
  const lat = Number.isFinite(Number(rawCenter[0])) ? Number(rawCenter[0]) : 39.9042;
  const lng = Number.isFinite(Number(rawCenter[1])) ? Number(rawCenter[1]) : 116.4074;
  const center: [number, number] = [lat, lng];
  const zoom = Number.isFinite(Number(config.zoom)) ? Number(config.zoom) : 10;
  const markers = config.markers || [];
  const geojson = config.geojson || null;
  const html = `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Map</title>
<link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
<style>
html,body{margin:0;padding:0;width:100%;height:100%}
#map{width:100%;height:100%}
</style>
</head>
<body>
<div id="map"></div>
<script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
<script>
(function(){
  var map = L.map('map').setView([${center[0]}, ${center[1]}], ${zoom});
  L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    attribution: '&copy; OpenStreetMap contributors'
  }).addTo(map);
  var markers = ${JSON.stringify(markers)};
  markers.forEach(function(m){
    var marker = L.marker([m.lat, m.lng]).addTo(map);
    if (m.popup) marker.bindPopup(m.popup).openPopup();
  });
  var geojson = ${JSON.stringify(geojson)};
  if (geojson) L.geoJSON(geojson).addTo(map);
})();
</script>
</body>
</html>`;
  const blob = new Blob([html], { type: "text/html" });
  return URL.createObjectURL(blob);
}

function MapView({ config }: { config: { center?: [number, number]; zoom?: number; markers?: Array<{ lat: number; lng: number; popup?: string }>; geojson?: unknown } }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    const url = buildMapIframe(config);
    setSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [config]);
  if (!src) return null;
  return <IframeView src={src} />;
}

function KatexView({ source, displayMode }: { source: string; displayMode?: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    if (!containerRef.current) return;
    try {
      katex.render(source, containerRef.current, {
        throwOnError: false,
        displayMode: displayMode ?? true,
      });
    } catch {
      setError("KaTeX 渲染失败");
    }
  }, [source, displayMode]);

  if (error) {
    return (
      <Alert severity="warning" sx={{ my: 1 }}>
        {error}
      </Alert>
    );
  }

  return (
    <Box
      ref={containerRef}
      sx={{
        width: "100%",
        overflowX: "auto",
        my: 1,
        p: 1.5,
        borderRadius: 1,
        border: (t) => `1px solid ${t.palette.divider}`,
        bgcolor: "background.paper",
        textAlign: "center",
      }}
    />
  );
}

export function VisualizationRenderer({
  config,
  onNodeClick,
}: {
  config: VizConfig;
  onNodeClick?: (text: string) => void;
}) {
  switch (config.type) {
    case "echarts":
      return <EChartView option={config.option} />;
    case "mermaid":
      return <MermaidFlow source={String(config.source || "")} onNodeClick={onNodeClick} />;
    case "pdb":
      return <PdbView url={String(config.url || "")} />;
    case "plotly":
      return <PlotlyView data={config.data} layout={config.layout} />;
    case "graphviz":
      return <DotFlow dot={String(config.dot || "")} onNodeClick={onNodeClick} />;
    case "three":
      return <ThreeView code={String(config.code || "")} />;
    case "map":
      return <MapView config={(config.config || {}) as { center?: [number, number]; zoom?: number; markers?: Array<{ lat: number; lng: number; popup?: string }>; geojson?: unknown }} />;
    case "katex":
      return <KatexView source={String(config.source || "")} displayMode={Boolean(config.displayMode ?? true)} />;
    case "iframe":
      return <IframeView src={String(config.url || "")} />;
    case "html":
      return <HtmlSandbox html={String(config.html || "")} />;
    default:
      return (
        <Alert severity="info" sx={{ my: 1 }}>
          未知可视化类型: {config.type}
        </Alert>
      );
  }
}
