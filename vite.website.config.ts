import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: "website",
  plugins: [react()],
  build: {
    outDir: "../dist-website",
    emptyOutDir: true,
  },
  server: {
    port: 4173,
    strictPort: true,
  },
});
