import { defineConfig } from 'vite'
import solid from 'vite-plugin-solid'

export default defineConfig({
  plugins: [solid()],
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
      '/rest/list': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        secure: false,
      },
      '/rest/session': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        secure: false,
      },
      '/rest/data': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        secure: false,
      }
    }
  }
})
