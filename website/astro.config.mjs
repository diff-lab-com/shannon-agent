import { defineConfig } from 'astro/config';
import react from '@astrojs/react';

export default defineConfig({
  site: 'https://shannon-agent.github.io',
  base: '/shannon-code',
  integrations: [react()],
  build: {
    format: 'directory',
  },
});
