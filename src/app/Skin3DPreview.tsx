import { useEffect, useRef, useState } from "react";
import type { SkinViewer as SkinViewerInstance } from "skinview3d";

type Props = { skinUrl: string; variant: string; capeUrl?: string; autoRotate?: boolean; className?: string };

export function Skin3DPreview({ skinUrl, variant, capeUrl, autoRotate = true, className = "" }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const viewerRef = useRef<SkinViewerInstance | null>(null);
  const [viewerReady, setViewerReady] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const host = hostRef.current;
    const canvas = canvasRef.current;
    if (!host || !canvas) return;
    let disposed = false;
    let viewer: SkinViewerInstance | null = null;
    let observer: ResizeObserver | null = null;
    void import("skinview3d").then(({ IdleAnimation, SkinViewer }) => {
      if (disposed) return;
      const width = Math.max(host.clientWidth, 260);
      const height = Math.max(host.clientHeight, 300);
      viewer = new SkinViewer({
        canvas,
        width,
        height,
        fov: 34,
        zoom: 0.78,
        background: 0x17181d,
        enableControls: true,
        pixelRatio: Math.min(window.devicePixelRatio || 1, 2),
      });
      viewer.animation = new IdleAnimation();
      viewer.autoRotate = autoRotate;
      viewerRef.current = viewer;
      observer = new ResizeObserver(() => {
        if (host.clientWidth > 0 && host.clientHeight > 0) viewer?.setSize(host.clientWidth, host.clientHeight);
      });
      observer.observe(host);
      setViewerReady(true);
    }).catch(() => { if (!disposed) setError("3D preview could not be initialized."); });
    return () => {
      disposed = true;
      observer?.disconnect();
      viewer?.dispose();
      if (viewerRef.current === viewer) viewerRef.current = null;
    };
  }, []);

  useEffect(() => {
    const viewer = viewerRef.current;
    if (!viewer || !viewerReady) return;
    let current = true;
    setError("");
    viewer.autoRotate = autoRotate;
    viewer.loadSkin(skinUrl, { model: variant.toLowerCase() === "slim" ? "slim" : "default" })
      .catch(() => { if (current) setError("Skin preview could not be loaded."); });
    if (capeUrl) viewer.loadCape(capeUrl).catch(() => { if (current) setError("Cape preview could not be loaded."); });
    else viewer.loadCape(null);
    return () => { current = false; };
  }, [skinUrl, variant, capeUrl, autoRotate, viewerReady]);

  return <div className={`skin-3d-preview ${className}`} ref={hostRef}>
    <canvas ref={canvasRef} aria-label="Interactive 3D Minecraft player preview" />
    {(!viewerReady || error) && <div className="skin-preview-error" role="status">{error || "Loading 3D model…"}</div>}
    <span className="skin-preview-hint">Drag to rotate</span>
  </div>;
}
