import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';
import solidPlugin from 'vite-plugin-solid';

// Set this flag to have our dev server proxy backend requests to the production live server.
let use_prod_server = true;

var target;
if (use_prod_server) {
  target = "http://35.232.163.159";
} else {
  target = 'http://localhost:3000';
}

export default defineConfig({
  plugins: [solidPlugin(), tailwindcss()],
  server: {
    port: 8080,
    proxy: {
      '/rest': {
        target: target,
        changeOrigin: true,
        secure: false,
      },
      '/data': {
        target: target,
        changeOrigin: true,
        secure: false,

        configure: (proxy, options) => {
          // proxy.on('error', (err, req, res) => {
          //   console.log('Proxy error:', err);
          // });
          // proxy.on('proxyReq', (proxyReq, req, res) => {
          //   console.log(`[${new Date().toISOString()}] PROXY REQ: ${req.method} ${req.url}`);
          //   console.log('  Headers:', JSON.stringify(req.headers, null, 2));
          //   req.on('close', () => {
          //     console.log(`[${new Date().toISOString()}] CLIENT REQ CLOSED: ${req.method} ${req.url}`);
          //   });
          // });
          // proxy.on('proxyRes', (proxyRes, req, res) => {
          //   console.log(`[${new Date().toISOString()}] PROXY RES: ${proxyRes.statusCode} ${req.url}`);
          //   console.log('  Headers:', JSON.stringify(proxyRes.headers, null, 2));
          // });
        }
      },
    }
  },
  build: {
    target: 'esnext',
  },
});
