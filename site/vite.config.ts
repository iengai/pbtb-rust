import { cpSync, existsSync, statSync, createReadStream } from "node:fs";
import { extname, join, resolve } from "node:path";
import type { IncomingMessage, ServerResponse } from "node:http";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

// The template backtests (`templates/`, committed) live at the project root
// rather than under `public/`, beside the script that generates them. This
// plugin makes the directory behave like `public/`: served verbatim in dev and
// preview, copied verbatim into `dist/` at build.
function staticDirs(dirs: string[]): Plugin {
  const root = resolve(__dirname);
  const types: Record<string, string> = {
    ".json": "application/json",
    ".html": "text/html",
    ".txt": "text/plain",
  };
  const serve = (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    const url = (req.url ?? "").split("?")[0];
    const base = process.env.VITE_BASE_PATH ?? "/";
    const rel = url.startsWith(base) ? url.slice(base.length) : url.replace(/^\//, "");
    const dir = dirs.find((d) => rel === d || rel.startsWith(`${d}/`));
    if (!dir) return next();
    const file = join(root, rel);
    if (!file.startsWith(join(root, dir)) || !existsSync(file) || !statSync(file).isFile()) {
      return next();
    }
    res.setHeader("Content-Type", types[extname(file)] ?? "application/octet-stream");
    res.setHeader("Cache-Control", "no-cache");
    createReadStream(file).pipe(res);
  };
  return {
    name: "pbtb-static-dirs",
    configureServer(server) {
      server.middlewares.use(serve);
    },
    configurePreviewServer(server) {
      server.middlewares.use(serve);
    },
    closeBundle() {
      const dist = join(root, "dist");
      if (!existsSync(dist)) return;
      for (const dir of dirs) {
        const src = join(root, dir);
        if (existsSync(src)) cpSync(src, join(dist, dir), { recursive: true });
      }
      // GitHub Pages serves 404.html for unknown paths; the SPA's own router
      // then resolves the deep link.
      cpSync(join(dist, "index.html"), join(dist, "404.html"));
    },
  };
}

// The site is served from the repository path on GitHub Pages and from the
// origin root in dev. The router and the OAuth redirect URI both derive from
// this value via `import.meta.env.BASE_URL`.
export default defineConfig(({ command, isPreview }) => {
  const base = command === "build" || isPreview ? "/pbtb-rust/" : "/";
  process.env.VITE_BASE_PATH = base;
  return {
    base,
    plugins: [react(), staticDirs(["templates"])],
    build: { outDir: "dist", emptyOutDir: true },
    server: { port: 5173, strictPort: true },
    preview: { port: 4173, strictPort: true },
  };
});
