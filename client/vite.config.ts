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
        configure: (proxy, options) => {
          proxy.on('error', (err, req, res) => {
            console.log('Proxy error:', err);
          });
          proxy.on('proxyReq', (proxyReq, req, res) => {
            console.log(`[${new Date().toISOString()}] PROXY REQ: ${req.method} ${req.url}`);
            console.log('  Headers:', JSON.stringify(req.headers, null, 2));
            req.on('close', () => {
              console.log(`[${new Date().toISOString()}] CLIENT REQ CLOSED: ${req.method} ${req.url}`);
            });
          });
          proxy.on('proxyRes', (proxyRes, req, res) => {
            console.log(`[${new Date().toISOString()}] PROXY RES: ${proxyRes.statusCode} ${req.url}`);
            console.log('  Headers:', JSON.stringify(proxyRes.headers, null, 2));
          });
        }
      },
    }
  }
})
