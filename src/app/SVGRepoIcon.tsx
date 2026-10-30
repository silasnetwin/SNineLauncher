import type { CSSProperties } from "react";

export function SVGRepoIcon({ src, className = "" }: { src: string; className?: string }) {
  const style = { "--svg-repo-icon": `url("${src}")` } as CSSProperties;
  return <span className={`svg-repo-icon ${className}`} style={style} aria-hidden="true" />;
}
