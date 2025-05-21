import { defineConfig } from 'vite'
import solid from 'vite-plugin-solid'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [solid(), tailwindcss()],
  optimizeDeps: {
    include: ['solid-js/web'],
  },
  resolve: {
    alias: {
      'solid-js/dom': 'solid-js/web',
    },
  },
  server: {
    proxy: {
      '/rest': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        secure: false,
      },
      '/data': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        secure: false,
      },
    }
  }
})
