import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    // Rust build artifacts can be locked while Tauri and Vite run together.
    watch: { ignored: ['**/target/**', '**/src-tauri/**'] },
  },
});
