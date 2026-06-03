import { defineConfig } from 'astro/config';
import react from '@astrojs/react';

// GitHub Pages needs /shannon-code base, dev uses root
const isCI = process.env.CI === 'true';

export default defineConfig({
  site: 'https://shannon-agent.github.io',
  base: isCI ? '/shannon-code' : '/',
  integrations: [react()],
  build: {
    format: 'directory',
  },
});
