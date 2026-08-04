import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const publishedContentUrl =
    env.VITE_PUBLISHED_CONTENT_URL ?? 'https://d81ul6xa7pt91.cloudfront.net'

  return {
    base: process.env.BASE_PATH ?? '/',
    plugins: [react()],
    // Local browser requests are same-origin, so developing the public reader
    // never depends on CloudFront CORS headers.
    server: {
      proxy: {
        '/content': {
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/content/, ''),
          target: publishedContentUrl,
        },
      },
    },
  }
})
